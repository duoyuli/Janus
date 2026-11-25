//! The run's usage ledger: a read-only summary derived from the event log, for the run, the lead
//! and collaborator subtotals, each task, member, attempt and session. The rules are in
//! `docs/interfaces/20261031-events.md#usage_accounting`; the app derives the same ledger from the same
//! events (`frontend/Sources/Janus/Core/UsageLedger.swift`), and `backend/testdata/usage_cases.json`
//! holds the cases both sides must agree on.
//!
//! - **Tokens** come only from `session/prompt` responses. Each one is the work of that prompt
//!   (ACP's `usage`; the Claude Code and Codex adapters start it again at every prompt), so the
//!   turns of a session are added up. When the response carries `_meta.quota.model_usage`, its rows
//!   also count subagents and compaction, and they replace `usage` for that turn.
//! - **Context** snapshots (`usage_update`'s `used` / `size`) are the window right now, not
//!   consumption: only the latest value, the peak and the ratio are kept, never added to tokens.
//! - **Cost** is a running total per session (`usage_update`'s `cost`): the latest reading counts.
//!   A reading below the previous one is a reset (`/clear`), so what was reached before it is kept;
//!   a zero after a positive reading is a crashed result and is ignored. Currencies never mix and
//!   nothing is estimated from tokens.
//! - **Missing stays `None`** and is written 「未上报」 / 「部分上报」, never 0.
//! - **Time** is wall-clock; waiting for a person is kept apart as `wait_ms`.
//! - The ledger is a pure function of the events, so a full read, a replay prefix, a restart and a
//!   historic run give the same numbers. It never changes scheduling, budgets, states or delivery,
//!   and its reminders change nothing.

use crate::db::Event;
use crate::events::EventKind;
use crate::lang::{Lang, tr};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};

/// Default reminder thresholds; the app keeps the same values in `usageReminders`.
/// The run reached this many tokens.
pub const TOKEN_REMINDER: i64 = 500_000;
/// One task holds at least this share of a run of at least [`TASK_SHARE_FLOOR`] tokens.
pub const TASK_SHARE: f64 = 0.6;
pub const TASK_SHARE_FLOOR: i64 = 50_000;
/// This many collaborators and no task succeeded within [`IDLE_WITHOUT_SUCCESS`].
pub const MANY_COLLABORATORS: usize = 5;
pub const IDLE_WITHOUT_SUCCESS: i64 = 30 * 60 * 1000;
/// The run used this many tokens and no session reported a cost.
pub const COST_BLIND_TOKENS: i64 = 100_000;

/// The lead's actor in the events.
pub const LEAD: &str = "lead";

/// Agent ids to display names, given by the caller (the ledger only knows actors).
pub type Names = HashMap<String, String>;

/// Whether a range's token numbers are all there.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Completeness {
    /// No session of the range reported tokens.
    #[default]
    Missing,
    /// Every session of the range reported tokens.
    Complete,
    /// Some sessions reported tokens, some did not.
    Partial,
}

impl Completeness {
    pub fn label_in(self, lang: Lang) -> &'static str {
        match self {
            Completeness::Missing => lang.pick("未上报", "not reported"),
            Completeness::Complete => lang.pick("完整", "complete"),
            Completeness::Partial => lang.pick("部分上报", "partly reported"),
        }
    }

    fn of(reported: usize, missing: usize) -> Completeness {
        match (reported, missing) {
            (0, _) => Completeness::Missing,
            (_, 0) => Completeness::Complete,
            _ => Completeness::Partial,
        }
    }
}

/// Token counts under the ACP names. `None` is "not reported", kept apart from 0.
/// `thought` is part of `output`; `cached_read` and `cached_write` are part of `total`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tokens {
    pub total: Option<i64>,
    pub input: Option<i64>,
    pub output: Option<i64>,
    pub cached_read: Option<i64>,
    pub cached_write: Option<i64>,
    pub thought: Option<i64>,
}

impl Tokens {
    /// One count block of a `session.usage` event (or one of its `models` rows). A block without
    /// `totalTokens` but with input or output gets the sum of its parts as total.
    fn read(v: &Value) -> Tokens {
        let n = |k: &str| v[k].as_i64();
        let mut t = Tokens {
            total: n("totalTokens"),
            input: n("inputTokens"),
            output: n("outputTokens"),
            cached_read: n("cachedReadTokens"),
            cached_write: n("cachedWriteTokens"),
            thought: n("thoughtTokens"),
        };
        if t.total.is_none() && (t.input.is_some() || t.output.is_some()) {
            t.total = [t.input, t.output, t.cached_read, t.cached_write].into_iter().flatten().reduce(|a, b| a + b);
        }
        t
    }

    /// The tokens of one turn by model: the quota rows when the agent sent them (they name their
    /// model), else `usage` on the session's current model (`None` as model). Empty when the event
    /// carries no token count (a context snapshot).
    pub(crate) fn of_turn(data: &Value) -> Vec<(Option<String>, Tokens)> {
        let rows: Vec<(Option<String>, Tokens)> = data["models"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|row| (row["model"].as_str().filter(|m| !m.is_empty()).map(str::to_owned), Tokens::read(row)))
            .filter(|(_, t)| t.total.is_some())
            .collect();
        if !rows.is_empty() {
            return rows;
        }
        let usage = Tokens::read(data);
        if usage.total.is_some() { vec![(None, usage)] } else { vec![] }
    }

    /// The share of prompt tokens read from the cache: cache reads over input plus cache reads and
    /// writes. `None` unless the agent reported both input and cache reads.
    pub fn cache_hit_rate(&self) -> Option<f64> {
        let (input, read) = (self.input?, self.cached_read?);
        let prompt = input + read + self.cached_write.unwrap_or(0);
        (prompt > 0).then(|| read as f64 / prompt as f64)
    }

    /// Add another range's counts; a count neither side reported stays unreported.
    pub fn add(&mut self, other: &Tokens) {
        let plus = |a: Option<i64>, b: Option<i64>| match (a, b) {
            (Some(a), Some(b)) => Some(a + b),
            (a, b) => a.or(b),
        };
        self.total = plus(self.total, other.total);
        self.input = plus(self.input, other.input);
        self.output = plus(self.output, other.output);
        self.cached_read = plus(self.cached_read, other.cached_read);
        self.cached_write = plus(self.cached_write, other.cached_write);
        self.thought = plus(self.thought, other.thought);
    }
}

/// Context window snapshots: the latest, the peak and the window size. Never added to tokens.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Context {
    pub used: Option<i64>,
    pub peak: Option<i64>,
    pub size: Option<i64>,
    pub reports: usize,
}

impl Context {
    /// One `usage_update`. A `used` of 0 or less says nothing yet, as on the session card.
    fn snapshot(&mut self, used: Option<i64>, size: Option<i64>) {
        self.reports += 1;
        if let Some(u) = used.filter(|u| *u > 0) {
            self.used = Some(u);
            self.peak = Some(self.peak.map_or(u, |p| p.max(u)));
        }
        if let Some(s) = size.filter(|s| *s > 0) {
            self.size = Some(s);
        }
    }

    /// Merge a newer range's snapshots: the peak is the larger one, the latest value the newer one.
    fn merge(&mut self, newer: &Context) {
        self.reports += newer.reports;
        self.peak = self.peak.max(newer.peak);
        self.used = newer.used.or(self.used);
        self.size = newer.size.or(self.size);
    }

    /// The latest snapshot as a share of the window.
    pub fn ratio(&self) -> Option<f64> {
        match (self.used, self.size) {
            (Some(used), Some(size)) if size > 0 => Some(used as f64 / size as f64),
            _ => None,
        }
    }
}

/// A reported cost in one currency, as the agent gave it. An empty currency means none was named.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CostLine {
    pub currency: String,
    pub amount: f64,
    pub sessions: usize,
}

/// A range's cost: one line per currency, added only within the same currency.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cost {
    pub lines: Vec<CostLine>,
    pub reported_sessions: usize,
    pub missing_sessions: usize,
}

impl Cost {
    pub fn reported(&self) -> bool {
        self.reported_sessions > 0
    }

    /// Add one session's cost lines; a session without any is counted as missing.
    pub fn add(&mut self, lines: &[CostLine]) {
        if lines.is_empty() {
            self.missing_sessions += 1;
            return;
        }
        self.reported_sessions += 1;
        for l in lines {
            // Amounts without a currency may be in different units: each stays a line of its own.
            match self.lines.iter_mut().find(|x| !l.currency.is_empty() && x.currency == l.currency) {
                Some(x) => {
                    x.amount += l.amount;
                    x.sessions += l.sessions;
                }
                None => self.lines.push(l.clone()),
            }
        }
        self.lines.sort_by(|a, b| (a.currency.is_empty(), &a.currency).cmp(&(b.currency.is_empty(), &b.currency)));
    }
}

#[derive(Hash, PartialEq, Eq)]
enum CostSession {
    Native(String),
    Attempt(Option<String>, Option<i64>),
}

/// Convert cumulative native-session readings into deltas before assigning them to tasks.
/// The running scheduler and historical bills consume the same ordered events.
#[derive(Default)]
pub(crate) struct SessionCosts {
    current: HashMap<(String, Option<String>, Option<i64>), String>,
    previous: HashMap<(String, CostSession, String), f64>,
}

impl SessionCosts {
    fn sid<'a>(&'a self, e: &'a Event, actor: &str) -> Option<&'a str> {
        e.data
            .as_ref()
            .and_then(|d| d["session"].as_str())
            .filter(|s| !s.is_empty())
            .or_else(|| self.current.get(&(actor.to_owned(), e.task.clone(), e.attempt)).map(String::as_str))
    }

    pub(crate) fn observe(&mut self, e: &Event) -> Option<(String, f64)> {
        let actor = session_actor(e);
        let d = e.data.as_ref().unwrap_or(&Value::Null);
        let sid = d["session"].as_str().filter(|s| !s.is_empty());
        match e.kind() {
            EventKind::AttemptStarted | EventKind::AgentStatus => {
                let owner = (actor.into(), e.task.clone(), e.attempt);
                if let Some(sid) = sid {
                    self.current.insert(owner, sid.into());
                } else if e.kind() == EventKind::AttemptStarted {
                    self.current.remove(&owner);
                }
                None
            }
            EventKind::SessionUsage => {
                let amount = d["cost"]["amount"].as_f64()?;
                let name = d["cost"]["currency"].as_str().unwrap_or("");
                let currency = if name.eq_ignore_ascii_case("USD") { "USD" } else { name };
                let session = match self.sid(e, actor) {
                    Some(id) => CostSession::Native(id.into()),
                    None => CostSession::Attempt(e.task.clone(), e.attempt),
                };
                let previous = self.previous.entry((actor.into(), session, currency.into())).or_default();
                let delta = if amount <= 0.0 && *previous > 0.0 {
                    0.0 // A zero after a positive reading is an abnormal result.
                } else {
                    let delta = if amount < *previous { amount } else { amount - *previous };
                    *previous = amount;
                    delta
                };
                Some((currency.into(), delta))
            }
            _ => None,
        }
    }
}

/// What one turn spent on one model. A turn the agent reported by model has a row per model.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Spend {
    pub ts: i64,
    pub model: String,
    pub tokens: Tokens,
    /// The first row of its turn, so a bill counts turns once.
    pub first: bool,
}

/// The model name of a turn the agent did not report by model and whose session named none.
pub const UNKNOWN_MODEL: &str = "未知模型";

/// Tokens and turns on one model, in a bill.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelLine {
    pub model: String,
    pub tokens: Tokens,
    pub turns: usize,
    /// USD at the model's list price (`prices::Table::price`); `None` when not priced.
    pub estimate: Option<f64>,
}

/// Who paid in a bill: a member, in a task's bill, or an Agent, in a period's bill. Its models are
/// sorted by tokens, most first.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BillGroup {
    pub id: String,
    pub name: String,
    pub tokens: Tokens,
    pub turns: usize,
    pub cost: Cost,
    pub models: Vec<ModelLine>,
    /// The priced models' estimates added up; `None` when none of them has a price.
    pub estimate: Option<f64>,
}

impl BillGroup {
    pub fn new(id: &str, name: &str) -> BillGroup {
        BillGroup { id: id.to_string(), name: name.to_string(), ..Default::default() }
    }

    pub fn spend(&mut self, s: &Spend) {
        self.tokens.add(&s.tokens);
        self.turns += usize::from(s.first);
        let line = match self.models.iter().position(|m| m.model == s.model) {
            Some(i) => &mut self.models[i],
            None => {
                self.models.push(ModelLine { model: s.model.clone(), ..Default::default() });
                self.models.last_mut().expect("just pushed")
            }
        };
        line.tokens.add(&s.tokens);
        line.turns += 1;
    }

    /// Most tokens first, for the groups of a bill and the models of a group.
    pub fn sort(groups: &mut [BillGroup]) {
        for g in groups.iter_mut() {
            g.models.sort_by(|a, b| b.tokens.total.cmp(&a.tokens.total).then_with(|| a.model.cmp(&b.model)));
        }
        groups.sort_by(|a, b| b.tokens.total.cmp(&a.tokens.total).then_with(|| a.name.cmp(&b.name)));
    }
}

/// One ACP session (or one lead session): the unit numbers are counted in.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    /// The session's identity in the log: the actor with its ACP session id, or with its task and
    /// attempt when no id is known.
    pub key: String,
    /// The ACP session id, when the log names one.
    #[serde(skip)]
    pub sid: Option<String>,
    pub actor: String,
    pub task: Option<String>,
    pub attempt: Option<i64>,
    pub tokens: Tokens,
    /// Turns that reported tokens.
    pub turns: usize,
    pub context: Context,
    pub cost: Vec<CostLine>,
    pub first_ms: i64,
    pub last_ms: i64,
    /// Each turn's tokens by model, for bills.
    #[serde(skip)]
    pub spend: Vec<Spend>,
    /// Each cost increment (time, currency, delta), for a bill over part of the session.
    #[serde(skip)]
    pub readings: Vec<(i64, String, f64)>,
}

impl Session {
    /// Deltas are attributed to the timestamp and task of the reading that earned them.
    pub fn cost_between(&self, from: i64, to: i64) -> Vec<CostLine> {
        let mut amounts = BTreeMap::new();
        for (ts, currency, delta) in &self.readings {
            if (from..to).contains(ts) {
                *amounts.entry(currency.clone()).or_insert(0.0) += delta;
            }
        }
        amounts.into_iter().map(|(currency, amount)| CostLine { currency, amount, sessions: 1 }).collect()
    }
}

/// How an attempt ended.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AttemptState {
    /// Started, no end yet.
    #[default]
    Unfinished,
    Succeeded,
    Failed,
    /// Its result did not pass the result check.
    VerificationFailed,
    /// An expired attempt's result was ignored; its usage still counts.
    Expired,
    Cancelled,
}

impl AttemptState {
    pub fn label_in(self, lang: Lang) -> &'static str {
        match self {
            AttemptState::Unfinished => lang.pick("未完成", "unfinished"),
            AttemptState::Succeeded => lang.pick("成功", "succeeded"),
            AttemptState::Failed => lang.pick("失败", "failed"),
            AttemptState::VerificationFailed => lang.pick("验证没通过", "failed its check"),
            AttemptState::Expired => lang.pick("已过期但仍计入", "expired, still counted"),
            AttemptState::Cancelled => lang.pick("已取消", "cancelled"),
        }
    }
}

/// What an attempt's time went to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptKind {
    /// The first attempt at a task that does the work.
    #[default]
    Work,
    /// Any attempt of an independent verification (`verifies`), its re-checks too.
    Verify,
    /// The work again: a retry after a failure, or the lead sending it back (`janus.retry`).
    Rework,
}

/// Where the collaborators' time went, from their attempts. Each sum is wall-clock per attempt,
/// so attempts that ran side by side add up to more than `span_ms`.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeSplit {
    pub work_ms: i64,
    pub verify_ms: i64,
    pub rework_ms: i64,
    /// From the first attempt's start to the last one's end.
    pub span_ms: i64,
}

impl TimeSplit {
    pub fn total_ms(&self) -> i64 {
        self.work_ms + self.verify_ms + self.rework_ms
    }

    /// `子任务 42 分（执行 20 分 · 验证 15 分 · 返工 7 分）`, or nothing before any attempt.
    pub fn text(&self) -> Option<String> {
        if self.total_ms() == 0 {
            return None;
        }
        let mut parts = vec![format!("执行 {}", duration(self.work_ms))];
        if self.verify_ms > 0 {
            parts.push(format!("验证 {}", duration(self.verify_ms)));
        }
        if self.rework_ms > 0 {
            parts.push(format!("返工 {}", duration(self.rework_ms)));
        }
        Some(format!("子任务 {}（{}）", duration(self.total_ms()), parts.join(" · ")))
    }
}

/// One attempt's usage and outcome.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Attempt {
    pub task: String,
    pub n: i64,
    pub kind: AttemptKind,
    pub actor: String,
    pub state: AttemptState,
    /// Why it ended, in one line (a failure, the result check, a cancel).
    pub note: Option<String>,
    pub started_ms: i64,
    pub ended_ms: Option<i64>,
    pub duration_ms: Option<i64>,
    pub tokens: Tokens,
    pub turns: usize,
    pub context: Context,
    pub cost: Cost,
    pub sessions: usize,
    pub completeness: Completeness,
}

/// One range: the run, the lead, the collaborators, a task, a member or an attempt.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Scope {
    pub id: String,
    pub tokens: Tokens,
    pub turns: usize,
    pub context: Context,
    pub cost: Cost,
    /// Wall-clock span; `None` when the range has no records, not 0.
    pub duration_ms: Option<i64>,
    /// Time spent waiting for a person or the lead; `None` when nothing was waited on.
    pub wait_ms: Option<i64>,
    pub sessions: usize,
    pub reported_sessions: usize,
    pub missing_sessions: usize,
    pub completeness: Completeness,
}

impl Scope {
    fn new(id: &str) -> Scope {
        Scope { id: id.to_string(), ..Default::default() }
    }

    /// The range over `sessions` (oldest first, so the latest context snapshot wins).
    fn over<'a>(
        id: &str,
        sessions: impl Iterator<Item = &'a Session>,
        duration_ms: Option<i64>,
        wait_ms: Option<i64>,
    ) -> Scope {
        let mut s = Scope::new(id);
        for session in sessions {
            s.sessions += 1;
            if session.turns > 0 {
                s.reported_sessions += 1;
            } else {
                s.missing_sessions += 1;
            }
            s.tokens.add(&session.tokens);
            s.turns += session.turns;
            s.context.merge(&session.context);
            s.cost.add(&session.cost);
        }
        s.completeness = Completeness::of(s.reported_sessions, s.missing_sessions);
        s.duration_ms = duration_ms;
        s.wait_ms = wait_ms;
        s
    }
}

/// A wait for a person or the lead, with the task and member it held up.
struct Wait {
    ms: i64,
    task: Option<String>,
    actor: String,
}

/// A run's whole ledger. Every number comes from the same events.
#[derive(Clone, Debug, Serialize)]
pub struct Ledger {
    /// The run in one line: `用量：15,400 tokens（部分上报） · 费用：未上报 · 耗时 3 分 12 秒（总时长）`.
    pub text: String,
    pub run: Scope,
    pub lead: Scope,
    pub collaborators: Scope,
    pub tasks: Vec<Scope>,
    pub members: Vec<Scope>,
    pub attempts: Vec<Attempt>,
    /// The collaborators' time by what it went to.
    pub time: TimeSplit,
    pub sessions: Vec<Session>,
    /// Read-only reminders. The app words them in its own language (`Reminder::text` is the
    /// orchestrator's wording).
    pub reminders: Vec<Reminder>,
}

impl Ledger {
    /// The ledger's sessions with ACP session id `sid`, oldest first: one per task that ran in it
    /// (a follow-up task that continues a session gets its own).
    pub fn sessions_of<'a>(&'a self, sid: &'a str) -> impl DoubleEndedIterator<Item = &'a Session> + 'a {
        self.sessions.iter().filter(move |s| s.sid.as_deref() == Some(sid))
    }

    pub fn task(&self, id: &str) -> Option<&Scope> {
        self.tasks.iter().find(|t| t.id == id)
    }

    /// `用量：… · 费用：… · 耗时 …`, as `team` reads it.
    pub fn summary_line(&self) -> String {
        format!("用量：{}", self.run_line())
    }

    /// `用量 … · 费用：… · 耗时 …`, as the delivery reads it.
    pub fn delivery_line(&self) -> String {
        format!("用量 {}", self.run_line())
    }

    fn run_line(&self) -> String {
        let mut line = format!(
            "{} · {} · 耗时 {}",
            tokens_text(&self.run.tokens, self.run.completeness, Lang::Zh),
            cost_text(&self.run.cost, Lang::Zh),
            duration_text(self.run.duration_ms, Lang::Zh)
        );
        if let Some(time) = self.time.text() {
            line.push_str(&format!(" · {time}"));
        }
        line
    }

    /// The usage part of `task_detail`: the task, its makeup and context, each attempt and the
    /// members that spent it.
    pub fn task_lines(&self, task: &str, names: &Names, lang: Lang) -> Vec<String> {
        let Some(scope) = self.task(task).filter(|s| s.sessions > 0) else {
            return vec![tr!(
                lang,
                "用量：未开始（子任务 {task} 还没有会话）",
                "Usage: not started (subtask {task} has no session yet)"
            )];
        };
        let (tokens, cost, took) = (
            tokens_text(&scope.tokens, scope.completeness, lang),
            cost_text(&scope.cost, lang),
            duration_text(scope.duration_ms, lang),
        );
        let waited = scope
            .wait_ms
            .map(|ms| {
                let ms = duration_in(ms, lang);
                tr!(lang, "（其中等待用户 {ms}）", " (of which waiting for the user {ms})")
            })
            .unwrap_or_default();
        let mut out = vec![tr!(
            lang,
            "用量：{tokens} · {cost} · 耗时 {took}{waited}",
            "Usage: {tokens} · {cost} · time {took}{waited}"
        )];
        out.extend(breakdown_text(&scope.tokens, lang));
        out.extend(context_text(&scope.context, lang));
        for a in self.attempts.iter().filter(|a| a.task == task) {
            let (n, actor, state) = (a.n, who(&a.actor, names, lang), a.state.label_in(lang));
            let tokens = tokens_text(&a.tokens, a.completeness, lang);
            let note = a.note.as_ref().map(|n| tr!(lang, "（{n}）", " ({n})")).unwrap_or_default();
            let took = duration_text(a.duration_ms, lang);
            let cost = if a.cost.reported() { format!(" · {}", cost_text(&a.cost, lang)) } else { String::new() };
            out.push(tr!(
                lang,
                "尝试 {n}：{actor} · {state} · {tokens}{note} · {took}{cost}",
                "Attempt {n}: {actor} · {state} · {tokens}{note} · {took}{cost}"
            ));
        }
        let missing = lang.pick("未上报", "not reported");
        for g in self.task_bill(task, names, lang) {
            let models: Vec<String> = g
                .models
                .iter()
                .map(|m| format!("{} {}", m.model, m.tokens.total.map(num).unwrap_or_else(|| missing.into())))
                .collect();
            let tokens = g.tokens.total.map(|n| format!("{} tokens", num(n))).unwrap_or_else(|| missing.into());
            let models = if models.is_empty() {
                String::new()
            } else {
                let models = models.join(lang.pick("、", ", "));
                tr!(lang, " · 模型 {models}", " · models {models}")
            };
            let name = &g.name;
            out.push(tr!(lang, "成员 {name}：{tokens}{models}", "Member {name}: {tokens}{models}"));
        }
        out
    }

    /// A task's bill: each member that worked on it, with its tokens by model and its cost, most
    /// tokens first. A member whose sessions reported nothing is listed with no models.
    pub fn task_bill(&self, task: &str, names: &Names, lang: Lang) -> Vec<BillGroup> {
        let mut groups: Vec<BillGroup> = vec![];
        for s in self.sessions.iter().filter(|s| s.task.as_deref() == Some(task)) {
            let i = match groups.iter().position(|g| g.id == s.actor) {
                Some(i) => i,
                None => {
                    groups.push(BillGroup::new(&s.actor, &who(&s.actor, names, lang)));
                    groups.len() - 1
                }
            };
            for spend in &s.spend {
                groups[i].spend(spend);
            }
            groups[i].cost.add(&s.cost);
        }
        BillGroup::sort(&mut groups);
        groups
    }
}

/// The ledger of a slice of the event log. The same events always give the same numbers.
pub fn ledger(events: &[Event]) -> Ledger {
    let mut scan = Scan::default();
    for event in events {
        scan.event(event);
    }
    scan.finish()
}

/// A range's tokens in words: the number with its completeness, or 「未上报」.
pub fn tokens_text(tokens: &Tokens, completeness: Completeness, lang: Lang) -> String {
    match tokens.total {
        Some(n) => {
            let (n, how) = (num(n), completeness.label_in(lang));
            tr!(lang, "{n} tokens（{how}）", "{n} tokens ({how})")
        }
        None => lang.pick("未上报", "not reported").into(),
    }
}

/// What the tokens are made of, as far as the agents said: `构成：输入 … · 缓存读取 … · 输出 …`.
/// A part that is 0 or was never reported is left out.
fn breakdown_text(tokens: &Tokens, lang: Lang) -> Option<String> {
    let parts: Vec<String> = [
        (lang.pick("输入", "input"), tokens.input),
        (lang.pick("缓存读取", "cache read"), tokens.cached_read),
        (lang.pick("缓存写入", "cache write"), tokens.cached_write),
        (lang.pick("输出", "output"), tokens.output),
    ]
    .into_iter()
    .filter_map(|(label, n)| n.filter(|n| *n > 0).map(|n| format!("{label} {}", num(n))))
    .collect();
    if parts.is_empty() {
        return None;
    }
    let thought = tokens
        .thought
        .filter(|t| *t > 0)
        .map(|t| {
            let t = num(t);
            tr!(lang, "（输出中推理 {t}）", " (reasoning within output {t})")
        })
        .unwrap_or_default();
    let parts = parts.join(" · ");
    Some(tr!(lang, "构成：{parts}{thought}", "Makeup: {parts}{thought}"))
}

fn cost_text(cost: &Cost, lang: Lang) -> String {
    let parts: Vec<String> = cost
        .lines
        .iter()
        .map(|l| {
            let amount = money(l.amount);
            match (l.currency.as_str(), l.sessions) {
                ("", _) => tr!(lang, "{amount}（未标注币种）", "{amount} (no currency given)"),
                (c, 1) => format!("{amount} {c}"),
                (c, n) => tr!(lang, "{amount} {c}（{n} 个会话）", "{amount} {c} ({n} sessions)"),
            }
        })
        .collect();
    match parts.len() {
        0 => lang.pick("费用：未上报", "cost: not reported").into(),
        1 if !cost.lines[0].currency.is_empty() => {
            let part = &parts[0];
            tr!(lang, "费用：{part}", "cost: {part}")
        }
        _ => {
            let parts = parts.join(" + ");
            tr!(
                lang,
                "费用：{parts}（币种或口径不同，未合并）",
                "cost: {parts} (different currencies or bases, not added up)"
            )
        }
    }
}

fn context_text(context: &Context, lang: Lang) -> Option<String> {
    if context.reports == 0 {
        return None;
    }
    let used = context.used.map(num).unwrap_or_else(|| lang.pick("未上报", "not reported").into());
    let mut line = match context.size {
        Some(size) => {
            let (size, ratio) =
                (num(size), context.ratio().map(percent).unwrap_or_else(|| lang.pick("没有比例", "no ratio").into()));
            tr!(lang, "上下文 {used} / {size}（{ratio}）", "Context {used} / {size} ({ratio})")
        }
        None => tr!(lang, "上下文 {used}", "Context {used}"),
    };
    if let (Some(peak), Some(used)) = (context.peak, context.used)
        && peak > used
    {
        let peak = num(peak);
        line.push_str(&tr!(lang, " · 峰值 {peak}", " · peak {peak}"));
    }
    Some(line)
}

fn duration_text(ms: Option<i64>, lang: Lang) -> String {
    match ms {
        Some(ms) => {
            let d = duration_in(ms, lang);
            tr!(lang, "{d}（总时长）", "{d} (total)")
        }
        None => lang.pick("没有时长记录", "no time recorded").into(),
    }
}

/// A wall-clock duration, written as the app writes it (`L.dur`).
pub fn duration(ms: i64) -> String {
    duration_in(ms, Lang::Zh)
}

/// A wall-clock duration in `lang`.
pub fn duration_in(ms: i64, lang: Lang) -> String {
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

/// A number with thousands separators; a missing number is written in words by the caller.
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

/// An amount as reported, to four decimals at most.
fn money(v: f64) -> String {
    format!("{}", (v * 10_000.0).round() / 10_000.0)
}

fn percent(ratio: f64) -> String {
    format!("{:.0}%", ratio * 100.0)
}

fn who(actor: &str, names: &Names, lang: Lang) -> String {
    if actor == LEAD {
        return lang.pick("负责人", "the lead").into();
    }
    names.get(actor).cloned().unwrap_or_else(|| actor.to_string())
}

/// Who owns the session an event speaks about: `attempt.started` is written by Janus for the agent
/// in `to`; every other event's actor owns its session.
fn session_actor(e: &Event) -> &str {
    match e.kind() {
        EventKind::AttemptStarted => e.to.as_deref().unwrap_or(&e.actor),
        _ => &e.actor,
    }
}

/// A session while it is being collected.
#[derive(Default)]
struct SessionBuild {
    sid: Option<String>,
    actor: String,
    task: Option<String>,
    attempt: Option<i64>,
    tokens: Tokens,
    turns: usize,
    context: Context,
    cost: BTreeMap<String, f64>,
    spend: Vec<Spend>,
    readings: Vec<(i64, String, f64)>,
    first_ms: i64,
    last_ms: i64,
}

/// An attempt while it is being collected.
struct AttemptBuild {
    task: String,
    n: i64,
    actor: String,
    state: AttemptState,
    note: Option<String>,
    started_ms: i64,
    ended_ms: Option<i64>,
    last_ms: i64,
    /// The lead sent the task back with `janus.retry`.
    sent_back: bool,
    /// It went back into a session a restart broke off: the same work as the attempt before it.
    restarted: bool,
}

/// One pass over the events.
#[derive(Default)]
struct Scan {
    created_ms: Option<i64>,
    last_ms: Option<i64>,
    /// Each actor's current ACP session, for the events that do not name one.
    costs: SessionCosts,
    /// Each actor's current model, from its session's controls.
    current_model: HashMap<String, String>,
    sessions: HashMap<String, SessionBuild>,
    order: Vec<String>,
    attempts: HashMap<(String, i64), AttemptBuild>,
    attempt_order: Vec<(String, i64)>,
    /// Tasks that verify another one (`verifies`).
    verifiers: HashSet<String>,
    tasks: Vec<String>,
    task_span: HashMap<String, (i64, i64)>,
    waits: Vec<Wait>,
    open_waits: HashMap<String, (i64, Option<String>, String)>,
    /// Collaborators the orchestration named, including ones that never opened a session.
    collaborators: HashSet<String>,
    run_cancelled: Option<i64>,
    task_cancelled: HashMap<String, i64>,
}

impl Scan {
    fn event(&mut self, e: &Event) {
        self.last_ms = Some(self.last_ms.map_or(e.ts, |l| l.max(e.ts)));
        if let Some(task) = &e.task {
            self.note_task(task);
            let span = self.task_span.entry(task.clone()).or_insert((e.ts, e.ts));
            span.0 = span.0.min(e.ts);
            span.1 = span.1.max(e.ts);
        }
        let empty = Value::Null;
        let data = e.data.as_ref().unwrap_or(&empty);
        let cost = self.costs.observe(e);
        match e.kind() {
            EventKind::RunCreated => self.created_ms = Some(e.ts),
            EventKind::AttemptStarted | EventKind::AgentStatus => {
                let actor = session_actor(e).to_string();
                if e.kind() == EventKind::AgentStatus && data["session"].as_str().is_none_or(str::is_empty) {
                    return;
                }
                // An opened session can be one whose usage never arrives: it counts as unreported.
                self.touch(e, &actor);
                if e.kind() == EventKind::AttemptStarted
                    && let (Some(task), Some(n)) = (&e.task, e.attempt)
                {
                    self.attempt(task, n, &actor, e.ts);
                    if let Some(a) = self.attempts.get_mut(&(task.clone(), n)) {
                        a.sent_back = data["reopen"]["reason"] == "lead.retry";
                        a.restarted = data["brokeOff"] == "restart";
                    }
                }
            }
            EventKind::SessionUsage => self.usage(e, data, cost),
            // The model a session runs on: as it opened, and when someone switched it.
            EventKind::SessionControls => {
                if let Some(model) = data["model"]["current"].as_str().filter(|m| !m.is_empty()) {
                    self.current_model.insert(e.actor.clone(), model.to_string());
                }
            }
            EventKind::SessionControl if data["id"] == "model" && data["failed"] != true => {
                if let Some(model) = data["value"].as_str().filter(|m| !m.is_empty()) {
                    self.current_model.insert(e.actor.clone(), model.to_string());
                }
            }
            EventKind::AttemptFailed => {
                self.attempt_ended(e, AttemptState::Failed, data["error"].as_str().map(str::to_owned))
            }
            EventKind::AttemptLateResult => {
                self.attempt_ended(e, AttemptState::Expired, Some("过期尝试的结果被忽略，用量照常计入".into()))
            }
            EventKind::VerificationFailed => {
                let failures: Vec<&str> =
                    data["failures"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
                self.attempt_ended(
                    e,
                    AttemptState::VerificationFailed,
                    (!failures.is_empty()).then(|| failures.join("；")),
                );
            }
            EventKind::TaskSucceeded => self.attempt_ended(e, AttemptState::Succeeded, None),
            EventKind::TaskFailed => {
                let note = match data["reason"].as_str() {
                    Some("verification") => Some("验收没通过".into()),
                    Some("attempts") => Some("用完尝试次数".into()),
                    _ => None,
                };
                self.attempt_ended(e, AttemptState::Failed, note);
            }
            // The lead cancelled one task; Janus writes no attempt event for it (see `finish`).
            EventKind::TaskSkippedByDecision => {
                if let Some(task) = &e.task {
                    let at = self.task_cancelled.entry(task.clone()).or_insert(e.ts);
                    *at = (*at).min(e.ts);
                }
            }
            EventKind::RunCancelled => self.run_cancelled = Some(self.run_cancelled.map_or(e.ts, |at| at.min(e.ts))),
            EventKind::OrchestrationAccepted => {
                for t in data["tasks"].as_array().into_iter().flatten() {
                    if let Some(id) = t["id"].as_str() {
                        self.note_task(id);
                        if t["verifies"].as_str().is_some_and(|v| !v.is_empty()) {
                            self.verifiers.insert(id.to_string());
                        }
                    }
                    if let Some(agent) = t["agent"].as_str() {
                        self.collaborators.insert(agent.to_string());
                    }
                }
            }
            // Requests that hold a session until a person or the lead answers.
            EventKind::QuestionAsked | EventKind::ApprovalRequested | EventKind::InputRequested => {
                if let Some(key) = data["key"].as_str().filter(|k| !k.is_empty()) {
                    self.open_waits.insert(key.to_string(), (e.ts, e.task.clone(), e.actor.clone()));
                }
            }
            EventKind::QuestionAnswered
            | EventKind::ApprovalGranted
            | EventKind::ApprovalDenied
            | EventKind::InputAnswered
            | EventKind::DecisionWithdrawn => {
                if let Some((start, task, actor)) = data["key"].as_str().and_then(|k| self.open_waits.remove(k)) {
                    self.waits.push(Wait { ms: e.ts.saturating_sub(start), task, actor });
                }
            }
            _ => {}
        }
    }

    fn note_task(&mut self, task: &str) {
        if !self.tasks.iter().any(|t| t == task) {
            self.tasks.push(task.to_string());
        }
    }

    fn attempt(&mut self, task: &str, n: i64, actor: &str, ts: i64) {
        let key = (task.to_string(), n);
        let a = self.attempts.entry(key.clone()).or_insert_with(|| {
            self.attempt_order.push(key);
            AttemptBuild {
                task: task.to_string(),
                n,
                actor: actor.to_string(),
                state: AttemptState::Unfinished,
                note: None,
                started_ms: ts,
                ended_ms: None,
                last_ms: ts,
                sent_back: false,
                restarted: false,
            }
        });
        a.last_ms = a.last_ms.max(ts);
        if actor != LEAD {
            self.collaborators.insert(actor.to_string());
        }
    }

    /// An attempt's outcome. Only events with a task and an attempt reach one; the lead has none.
    fn attempt_ended(&mut self, e: &Event, state: AttemptState, note: Option<String>) {
        let (Some(task), Some(n)) = (&e.task, e.attempt) else { return };
        self.attempt(task, n, &e.actor, e.ts);
        if let Some(a) = self.attempts.get_mut(&(task.clone(), n)) {
            a.state = state;
            a.note = note.or(a.note.take());
            a.ended_ms = Some(a.ended_ms.map_or(e.ts, |t| t.max(e.ts)));
        }
    }

    /// One `session.usage`: a turn's tokens, a context snapshot, a cost reading, or several.
    fn usage(&mut self, e: &Event, data: &Value, cost: Option<(String, f64)>) {
        let rows = Tokens::of_turn(data);
        let (used, size) = (data["used"].as_i64(), data["size"].as_i64());
        if rows.is_empty() && used.is_none() && size.is_none() && cost.is_none() {
            return;
        }
        let actor = session_actor(e).to_string();
        let model = self.current_model.get(&actor).cloned().unwrap_or_else(|| UNKNOWN_MODEL.into());
        let key = self.touch(e, &actor);
        let s = self.sessions.get_mut(&key).expect("touched above");
        if !rows.is_empty() {
            s.turns += 1;
        }
        for (i, (named, tokens)) in rows.into_iter().enumerate() {
            s.tokens.add(&tokens);
            s.spend.push(Spend { ts: e.ts, model: named.unwrap_or_else(|| model.clone()), tokens, first: i == 0 });
        }
        if used.is_some() || size.is_some() {
            s.context.snapshot(used, size);
        }
        if let Some((currency, amount)) = cost {
            *s.cost.entry(currency.clone()).or_default() += amount;
            s.readings.push((e.ts, currency, amount));
        }
    }

    /// Note that the actor is in its current session, even if it never reports usage, and return
    /// the session's key. Scoped by task as well as session id so a downstream task that warms up
    /// from an earlier task's ACP session (`continues`) gets its own slice of turns in the ledger,
    /// and each attempt retains its own share.
    fn touch(&mut self, e: &Event, actor: &str) -> String {
        let task_part = e.task.as_deref().unwrap_or("-");
        let sid = self.costs.sid(e, actor).map(str::to_owned);
        let key = match &sid {
            Some(sid) => format!("{actor}:{task_part}/{n}:{sid}", n = e.attempt.unwrap_or_default()),
            None => format!("{actor}:{task_part}/{}", e.attempt.map(|n| n.to_string()).unwrap_or_else(|| "-".into())),
        };
        let s = self.sessions.entry(key.clone()).or_insert_with(|| {
            self.order.push(key.clone());
            SessionBuild {
                sid,
                actor: actor.to_string(),
                task: e.task.clone(),
                attempt: e.attempt,
                first_ms: e.ts,
                last_ms: e.ts,
                ..Default::default()
            }
        });
        s.first_ms = s.first_ms.min(e.ts);
        s.last_ms = s.last_ms.max(e.ts);
        if actor != LEAD {
            self.collaborators.insert(actor.to_string());
        }
        key
    }

    fn finish(mut self) -> Ledger {
        let end = self.last_ms.unwrap_or_default();
        // Janus kills the sessions of a cancelled run (or of a task the lead cancels) just before
        // it records the cancel, so a failure event can still follow it. An attempt still running
        // when its cancel arrived was cancelled; what it reported stays counted.
        for ((task, _), a) in self.attempts.iter_mut() {
            let task_at = self.task_cancelled.get(task).copied();
            let (at, note) = match (self.run_cancelled, task_at) {
                (Some(run), Some(task)) if task <= run => (task, "负责人取消了这个子任务"),
                (Some(run), _) => (run, "任务被取消"),
                (None, Some(task)) => (task, "负责人取消了这个子任务"),
                (None, None) => continue,
            };
            if a.state == AttemptState::Succeeded || a.ended_ms.is_some_and(|end| end < at) {
                continue;
            }
            a.state = AttemptState::Cancelled;
            a.note = Some(note.into());
            a.ended_ms = Some(a.ended_ms.map_or(at, |end| end.max(at)));
        }
        // An attempt with no end of its own (a restart cut it off) ran until the next attempt at
        // its task started.
        let mut next_start: HashMap<&str, i64> = HashMap::new();
        for key in self.attempt_order.iter().rev() {
            let Some(a) = self.attempts.get_mut(key) else { continue };
            if a.ended_ms.is_none()
                && let Some(next) = next_start.get(key.0.as_str())
            {
                a.ended_ms = Some(*next);
            }
            next_start.insert(key.0.as_str(), a.started_ms);
        }
        // A request nobody answered yet waits until the last event.
        for (start, task, actor) in std::mem::take(&mut self.open_waits).into_values() {
            self.waits.push(Wait { ms: end.saturating_sub(start), task, actor });
        }
        let waits = &self.waits;
        let wait_of = |pred: &dyn Fn(&Wait) -> bool| -> Option<i64> {
            waits.iter().filter(|w| pred(w)).map(|w| w.ms).reduce(|a, b| a + b)
        };

        let mut sessions: Vec<Session> = self
            .order
            .iter()
            .filter_map(|key| self.sessions.remove(key).map(|b| (key, b)))
            .map(|(key, b)| Session {
                key: key.clone(),
                sid: b.sid,
                actor: b.actor,
                task: b.task,
                attempt: b.attempt,
                tokens: b.tokens,
                turns: b.turns,
                context: b.context,
                cost: b.cost.into_iter().map(|(currency, amount)| CostLine { currency, amount, sessions: 1 }).collect(),
                first_ms: b.first_ms,
                last_ms: b.last_ms,
                spend: b.spend,
                readings: b.readings,
            })
            .collect();
        // Oldest first, so a range's latest context snapshot is the one that shows.
        sessions.sort_by_key(|s| s.last_ms);

        let run =
            Scope::over("run", sessions.iter(), self.created_ms.map(|c| end.saturating_sub(c)), wait_of(&|_| true));
        let lead = Scope::over(LEAD, sessions.iter().filter(|s| s.actor == LEAD), None, wait_of(&|w| w.actor == LEAD));
        let collaborators = Scope::over(
            "collaborators",
            sessions.iter().filter(|s| s.actor != LEAD),
            None,
            wait_of(&|w| w.actor != LEAD),
        );
        let tasks: Vec<Scope> = self
            .tasks
            .iter()
            .map(|task| {
                let span = self.task_span.get(task).map(|(a, b)| b.saturating_sub(*a));
                Scope::over(
                    task,
                    sessions.iter().filter(|s| s.task.as_deref() == Some(task.as_str())),
                    span,
                    wait_of(&|w| w.task.as_deref() == Some(task.as_str())),
                )
            })
            .collect();
        let mut actors: Vec<&str> = sessions.iter().map(|s| s.actor.as_str()).collect();
        actors.sort();
        actors.dedup();
        let members: Vec<Scope> = actors
            .iter()
            .map(|actor| {
                Scope::over(actor, sessions.iter().filter(|s| s.actor == *actor), None, wait_of(&|w| w.actor == *actor))
            })
            .collect();
        // What each attempt's time went to, in the order they started: a verification's are all
        // checking; the first attempt at a task is work, any later one rework, except one that
        // only went on after a restart, which is whatever the attempt before it was.
        let mut kinds: HashMap<(String, i64), AttemptKind> = HashMap::new();
        let mut last: HashMap<&str, AttemptKind> = HashMap::new();
        for key in &self.attempt_order {
            let Some(b) = self.attempts.get(key) else { continue };
            let kind = if self.verifiers.contains(&b.task) {
                AttemptKind::Verify
            } else if b.restarted {
                last.get(b.task.as_str()).copied().unwrap_or_default()
            } else if b.n > 1 || b.sent_back {
                AttemptKind::Rework
            } else {
                AttemptKind::Work
            };
            last.insert(&b.task, kind);
            kinds.insert(key.clone(), kind);
        }
        let attempts: Vec<Attempt> = self
            .attempt_order
            .iter()
            .filter_map(|key| self.attempts.get(key))
            .map(|b| {
                let scope = Scope::over(
                    "",
                    sessions.iter().filter(|s| s.task.as_deref() == Some(b.task.as_str()) && s.attempt == Some(b.n)),
                    None,
                    None,
                );
                Attempt {
                    task: b.task.clone(),
                    n: b.n,
                    kind: kinds.get(&(b.task.clone(), b.n)).copied().unwrap_or_default(),
                    actor: b.actor.clone(),
                    state: b.state,
                    note: b.note.clone(),
                    started_ms: b.started_ms,
                    ended_ms: b.ended_ms,
                    duration_ms: b
                        .ended_ms
                        .unwrap_or(b.last_ms)
                        .checked_sub(b.started_ms)
                        .filter(|ms| b.ended_ms.is_some() || *ms > 0),
                    tokens: scope.tokens,
                    turns: scope.turns,
                    context: scope.context,
                    cost: scope.cost,
                    sessions: scope.sessions,
                    completeness: scope.completeness,
                }
            })
            .collect();

        let time = time_split(&attempts);
        let mut ledger = Ledger {
            text: String::new(),
            run,
            lead,
            collaborators,
            tasks,
            members,
            attempts,
            time,
            sessions,
            reminders: vec![],
        };
        ledger.reminders = reminders(&ledger, self.collaborators.len());
        ledger.text = ledger.summary_line();
        ledger
    }
}

/// The collaborators' time by what each attempt went to, and the span they covered.
fn time_split(attempts: &[Attempt]) -> TimeSplit {
    let mut t = TimeSplit::default();
    for a in attempts {
        let ms = a.duration_ms.unwrap_or_default();
        match a.kind {
            AttemptKind::Work => t.work_ms += ms,
            AttemptKind::Verify => t.verify_ms += ms,
            AttemptKind::Rework => t.rework_ms += ms,
        }
    }
    let start = attempts.iter().map(|a| a.started_ms).min();
    let end = attempts.iter().map(|a| a.started_ms + a.duration_ms.unwrap_or_default()).max();
    if let (Some(start), Some(end)) = (start, end) {
        t.span_ms = end - start;
    }
    t
}

/// A read-only reminder about a run's usage, with the numbers behind it. Nothing changes because
/// of one: not the run, its attempts or the inbox.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Reminder {
    /// The run reported `total` tokens, past the default `threshold`.
    Tokens { total: i64, threshold: i64 },
    /// One task used `share` of the run's `total`.
    OneTask { task: String, total: i64, share: f64 },
    /// `collaborators` worked `minutes` without a task succeeding.
    IdleTeam { collaborators: usize, minutes: i64 },
    /// Tokens were reported, cost by no session.
    CostUnknown,
}

/// The read-only reminders, with the default thresholds above.
fn reminders(ledger: &Ledger, declared_collaborators: usize) -> Vec<Reminder> {
    let mut out = vec![];
    let total = ledger.run.tokens.total;
    if let Some(n) = total.filter(|n| *n >= TOKEN_REMINDER) {
        out.push(Reminder::Tokens { total: n, threshold: TOKEN_REMINDER });
    }
    if let Some(n) = total.filter(|n| *n >= TASK_SHARE_FLOOR)
        && let Some((task, used)) =
            ledger.tasks.iter().filter_map(|t| t.tokens.total.map(|v| (t, v))).max_by_key(|(_, v)| *v)
        && used as f64 >= n as f64 * TASK_SHARE
    {
        out.push(Reminder::OneTask { task: task.id.clone(), total: n, share: used as f64 / n as f64 });
    }
    let collaborators = declared_collaborators.max(ledger.members.iter().filter(|m| m.id != LEAD).count());
    if collaborators >= MANY_COLLABORATORS
        && !ledger.attempts.iter().any(|a| a.state == AttemptState::Succeeded)
        && ledger.run.duration_ms.is_some_and(|ms| ms >= IDLE_WITHOUT_SUCCESS)
    {
        out.push(Reminder::IdleTeam { collaborators, minutes: IDLE_WITHOUT_SUCCESS / 60_000 });
    }
    if total.is_some_and(|n| n >= COST_BLIND_TOKENS) && !ledger.run.cost.reported() {
        out.push(Reminder::CostUnknown);
    }
    out
}

/// Reads `backend/testdata/usage_cases.json` events the way the API writes them.
#[cfg(test)]
pub fn event_from_json(run: &str, v: &Value) -> Event {
    Event {
        run_id: run.into(),
        seq: v["seq"].as_i64().unwrap_or_default(),
        ts: v["ts"].as_i64().unwrap_or_default(),
        kind: v["type"].as_str().unwrap_or_default().into(),
        actor: v["actor"].as_str().unwrap_or_default().into(),
        to: v["to"].as_str().map(str::to_owned),
        task: v["task"].as_str().map(str::to_owned),
        attempt: v["attempt"].as_i64(),
        text: String::new(),
        data: v.get("data").cloned(),
    }
}

#[cfg(test)]
mod tests;
