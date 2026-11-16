//! Model prices from models.dev, so the usage bill can show what the tokens would cost at API list
//! prices. Janus downloads `https://models.dev/api.json` (data only: nothing in it is executed),
//! keeps one price per model in `<home>/prices/models.json`, checks every hour and downloads again
//! once a day. Decision and trust boundary: `docs/decisions/model_prices.md`,
//! `docs/operations/security.md#model_prices`.
//!
//! models.dev lists a model under its lab and under many hosts and resellers, at different prices.
//! The lab that makes a model family lists its API price; a model its lab does not list takes the
//! price most providers list it at, leaving out free listings. Prices are USD per million tokens;
//! context tiers above the base price are not applied.

use crate::usage::{BillGroup, Tokens};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::Duration;

pub const SOURCE: &str = "https://models.dev/api.json";
/// How often the background loop looks at the table's age.
pub const CHECK_EVERY: Duration = Duration::from_secs(60 * 60);
/// A table older than this is downloaded again.
const SYNC_AFTER_MS: i64 = 24 * 60 * 60 * 1000;
/// The download is refused above this size (the list is about 7 MB in 2026-11).
const MAX_BYTES: u64 = 64 * 1024 * 1024;
/// Model families and the models.dev provider of the lab that makes them: its listing is the API
/// price. Hosts list other labs' models too (NVIDIA lists DeepSeek for free), so the family decides.
const LABS: [(&str, &str); 23] = [
    ("claude", "anthropic"),
    ("gpt", "openai"),
    ("o1", "openai"),
    ("o3", "openai"),
    ("o4", "openai"),
    ("codex", "openai"),
    ("gemini", "google"),
    ("gemma", "google"),
    ("grok", "xai"),
    ("deepseek", "deepseek"),
    ("mistral", "mistral"),
    ("codestral", "mistral"),
    ("devstral", "mistral"),
    ("magistral", "mistral"),
    ("qwen", "alibaba"),
    ("qwq", "alibaba"),
    ("qvq", "alibaba"),
    ("kimi", "moonshotai"),
    ("glm", "zai"),
    ("minimax", "minimax"),
    ("llama", "meta"),
    ("command", "cohere"),
    ("nemotron", "nvidia"),
];

/// USD per million tokens.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Price {
    pub input: f64,
    pub output: f64,
    pub cache_read: Option<f64>,
    pub cache_write: Option<f64>,
}

impl Price {
    /// What `tokens` cost at this price, or `None` when the agent reported neither input nor output
    /// (a total alone cannot be priced). A cache price the list leaves out is the input price.
    pub fn cost(&self, t: &Tokens) -> Option<f64> {
        if t.input.is_none() && t.output.is_none() {
            return None;
        }
        let at = |n: Option<i64>, usd: f64| n.unwrap_or(0) as f64 * usd / 1e6;
        Some(
            at(t.input, self.input)
                + at(t.output, self.output)
                + at(t.cached_read, self.cache_read.unwrap_or(self.input))
                + at(t.cached_write, self.cache_write.unwrap_or(self.input)),
        )
    }

    /// A models.dev `cost` object; `None` without valid input and output prices.
    fn read(c: &Value) -> Option<Price> {
        let usd = |k: &str| c[k].as_f64().filter(|v| v.is_finite() && *v >= 0.0);
        Some(Price {
            input: usd("input")?,
            output: usd("output")?,
            cache_read: usd("cache_read"),
            cache_write: usd("cache_write"),
        })
    }
}

/// One model's price and where it came from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    #[serde(flatten)]
    pub price: Price,
    pub provider: String,
    /// The lab's own listing, rather than the price most providers list.
    pub official: bool,
}

/// The prices Janus keeps: one per model, by [`key`].
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Table {
    pub source: String,
    pub fetched_at: i64,
    pub models: BTreeMap<String, Entry>,
}

impl Table {
    /// One price per model from models.dev's `api.json` (provider → models → cost).
    pub fn from_models_dev(api: &Value, fetched_at: i64) -> Table {
        let mut listed: BTreeMap<String, Vec<(&str, Price)>> = BTreeMap::new();
        for (provider, p) in api.as_object().into_iter().flatten() {
            for (id, m) in p["models"].as_object().into_iter().flatten() {
                if let Some(price) = Price::read(&m["cost"]) {
                    listed.entry(key(id)).or_default().push((provider.as_str(), price));
                }
            }
        }
        let models = listed
            .into_iter()
            .filter_map(|(model, list)| {
                let lab = LABS
                    .iter()
                    .find(|(family, _)| model.starts_with(family))
                    .and_then(|(_, lab)| list.iter().find(|(p, _)| p == lab));
                let entry = match lab {
                    Some((provider, price)) => Entry { price: *price, provider: provider.to_string(), official: true },
                    None => {
                        let (provider, price) = most_listed(&list)?;
                        Entry { price, provider: provider.to_string(), official: false }
                    }
                };
                Some((model, entry))
            })
            .collect();
        Table { source: SOURCE.into(), fetched_at, models }
    }

    pub fn get(&self, model: &str) -> Option<&Entry> {
        self.models.get(&key(model))
    }

    /// Price a bill's models: each model line and each group gets an estimate where the model has a
    /// price, and the result says what could not be priced.
    pub fn price(&self, groups: &mut [BillGroup]) -> Estimate {
        let mut e = Estimate { source: self.source.clone(), fetched_at: self.fetched_at, ..Default::default() };
        for g in groups.iter_mut() {
            for m in g.models.iter_mut() {
                m.estimate = self.get(&m.model).and_then(|p| p.price.cost(&m.tokens));
                match m.estimate {
                    Some(usd) => {
                        g.estimate = Some(g.estimate.unwrap_or(0.0) + usd);
                        e.amount += usd;
                    }
                    None => {
                        e.unpriced_turns += m.turns;
                        if !e.unpriced_models.contains(&m.model) {
                            e.unpriced_models.push(m.model.clone());
                        }
                    }
                }
            }
        }
        e
    }
}

/// What a bill's tokens would cost at list prices: never an invoice.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Estimate {
    pub amount: f64,
    /// Turns on models without a price, left out of `amount`.
    pub unpriced_turns: usize,
    pub unpriced_models: Vec<String>,
    pub source: String,
    pub fetched_at: i64,
}

/// The price most providers list a model at, leaving out free listings unless every one is free;
/// on a tie, the cheaper input.
fn most_listed<'a>(list: &[(&'a str, Price)]) -> Option<(&'a str, Price)> {
    let paid: Vec<(&str, Price)> = list.iter().filter(|(_, p)| p.input > 0.0 || p.output > 0.0).copied().collect();
    let list = if paid.is_empty() { list } else { &paid };
    let mut count: HashMap<(u64, u64), usize> = HashMap::new();
    for (_, p) in list {
        *count.entry((p.input.to_bits(), p.output.to_bits())).or_default() += 1;
    }
    list.iter()
        .max_by(|(pa, a), (pb, b)| {
            let (ca, cb) =
                (count[&(a.input.to_bits(), a.output.to_bits())], count[&(b.input.to_bits(), b.output.to_bits())]);
            ca.cmp(&cb).then(b.input.total_cmp(&a.input)).then(pb.cmp(pa))
        })
        .copied()
}

/// How a model is looked up: lowercase, without a `[1m]`-style suffix or a `provider/` path, so
/// `codex/gpt-6-astra` and `magpie/remote/auto/deepseek-v4.1-flash` find `gpt-6-astra` and
/// `deepseek-v4.1-flash`.
pub fn key(model: &str) -> String {
    let m = model.trim().to_lowercase();
    let m = m.split('[').next().unwrap_or_default();
    m.rsplit('/').next().unwrap_or_default().trim().to_string()
}

fn path(home: &Path) -> PathBuf {
    home.join("prices").join("models.json")
}

/// The price table in use, kept in memory once read from its file or synced. `App` holds it,
/// next to the home its file lives in.
#[derive(Default)]
pub struct PriceBook(RwLock<Option<Arc<Table>>>);

impl crate::orchestrator::App {
    /// The table in use: in memory, else the one saved in Janus home, else none yet.
    pub fn prices(&self) -> Option<Arc<Table>> {
        if let Some(t) = self.prices.0.read().unwrap().clone() {
            return Some(t);
        }
        let t: Table = serde_json::from_slice(&std::fs::read(path(&self.cfg.data_dir)).ok()?).ok()?;
        let t = Arc::new(t);
        *self.prices.0.write().unwrap() = Some(t.clone());
        Some(t)
    }

    /// Download the list and replace the table when `force`, or when it is missing or a day old.
    /// The table in use stays when anything fails.
    pub async fn sync_prices(&self, force: bool) -> Result<Arc<Table>> {
        let now = crate::orchestrator::now_ms();
        if !force && let Some(t) = self.prices().filter(|t| now - t.fetched_at < SYNC_AFTER_MS) {
            return Ok(t);
        }
        let home = self.cfg.data_dir.clone();
        let table = Arc::new(tokio::task::spawn_blocking(move || fetch(&home, now)).await??);
        *self.prices.0.write().unwrap() = Some(table.clone());
        crate::config::log(format!("janus: model prices from {SOURCE}: {} models", table.models.len()));
        Ok(table)
    }
}

/// `curl` over HTTPS only, with a time and a size limit, into the prices directory; the new table
/// replaces the old one in a single rename.
fn fetch(home: &Path, now: i64) -> Result<Table> {
    let dir = home.join("prices");
    std::fs::create_dir_all(&dir)?;
    let raw = dir.join("api.json.part");
    let status = std::process::Command::new("curl")
        .args([
            "-fsSL",
            "--proto",
            "=https",
            "--tlsv1.2",
            "--max-time",
            "120",
            "--max-filesize",
            &MAX_BYTES.to_string(),
            "-o",
        ])
        .arg(&raw)
        .arg(SOURCE)
        .status()
        .context("运行 curl 失败")?;
    let bytes = std::fs::read(&raw);
    let _ = std::fs::remove_file(&raw);
    if !status.success() {
        bail!("下载 {SOURCE} 失败（curl {status}）");
    }
    let api: Value = serde_json::from_slice(&bytes?).context("models.dev 返回的不是 JSON")?;
    let table = Table::from_models_dev(&api, now);
    if table.models.is_empty() {
        bail!("models.dev 的列表里没有读到任何价格");
    }
    let tmp = dir.join("models.json.part");
    std::fs::write(&tmp, serde_json::to_vec(&table)?)?;
    std::fs::rename(&tmp, path(home))?;
    Ok(table)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::ModelLine;
    use serde_json::json;

    /// The shape of models.dev's `api.json`, trimmed: a lab and resellers listing the same models.
    fn api() -> Value {
        json!({
            "openai": { "id": "openai", "models": {
                "gpt-6-astra": { "id": "gpt-6-astra", "cost": { "input": 10, "output": 50, "cache_read": 1, "cache_write": 12.5,
                    "tiers": [{ "input": 20, "output": 75, "tier": { "type": "context", "size": 272000 } }] } },
                "gpt-image-2": { "id": "gpt-image-2" }
            } },
            "requesty": { "id": "requesty", "models": {
                "openai/gpt-6-astra": { "cost": { "input": 8, "output": 40 } },
                "deepseek-v4.1-flash": { "cost": { "input": 0.3, "output": 1.2, "cache_read": 0.006 } }
            } },
            "aimlapi": { "id": "aimlapi", "models": { "deepseek-v4.1-flash": { "cost": { "input": 0.3, "output": 1.2 } } } },
            "logits": { "id": "logits", "models": {
                "deepseek-v4.1-flash": { "cost": { "input": 0.15, "output": 0.6 } },
                "broken": { "cost": { "input": -1, "output": 2 } }
            } },
            // A host listing another lab's model for free, and the lab without that model.
            "nvidia": { "id": "nvidia", "models": { "deepseek-v4.1-flash": { "cost": { "input": 0, "output": 0 } } } },
            "fwd": { "id": "fwd", "models": { "deepseek-v4.1-flash": { "cost": { "input": 0, "output": 0 } } } },
            "deepseek": { "id": "deepseek", "models": { "deepseek-v4-flash": { "cost": { "input": 0.14, "output": 0.28 } } } }
        })
    }

    #[test]
    fn the_lab_price_wins_and_other_models_take_the_most_listed_price() {
        let t = Table::from_models_dev(&api(), 7);
        let astra = t.get("codex/gpt-6-astra").unwrap();
        assert_eq!(
            (astra.price.input, astra.price.cache_write, astra.provider.as_str(), astra.official),
            (10.0, Some(12.5), "openai", true)
        );
        let flash = t.get("magpie/remote-magpie/auto/DeepSeek-V4.1-Flash").unwrap();
        assert_eq!(
            (flash.price.input, flash.price.output, flash.official),
            (0.3, 1.2, false),
            "two paid providers list 0.3 / 1.2; the free listings and a host are not the lab"
        );
        assert!(t.get("deepseek-v4-flash").unwrap().official, "the lab's own model");
        assert!(
            t.get("gpt-image-2").is_none() && t.get("broken").is_none(),
            "no price, or an invalid one, is no entry"
        );
        assert_eq!(key("Claude-Opus-5-5[1m]"), "claude-opus-5-5");
    }

    #[test]
    fn tokens_are_priced_per_million_and_the_unpriced_are_counted() {
        let p = Price { input: 10.0, output: 50.0, cache_read: Some(1.0), cache_write: None };
        let t = Tokens {
            total: Some(2_300_000),
            input: Some(1_000_000),
            output: Some(100_000),
            cached_read: Some(1_000_000),
            cached_write: Some(200_000),
            thought: None,
        };
        assert!(
            (p.cost(&t).unwrap() - (10.0 + 5.0 + 1.0 + 2.0)).abs() < 1e-9,
            "a cache write without its own price is charged as input"
        );
        assert_eq!(p.cost(&Tokens { total: Some(5), ..Default::default() }), None, "a total alone cannot be priced");

        let table = Table::from_models_dev(&api(), 7);
        let mut groups = vec![BillGroup {
            models: vec![
                ModelLine {
                    model: "gpt-6-astra".into(),
                    tokens: Tokens { input: Some(2_000_000), output: Some(0), ..Default::default() },
                    turns: 3,
                    estimate: None,
                },
                ModelLine {
                    model: "mystery".into(),
                    tokens: Tokens { input: Some(5), ..Default::default() },
                    turns: 2,
                    estimate: None,
                },
            ],
            ..BillGroup::new("codex", "Codex")
        }];
        let e = table.price(&mut groups);
        assert_eq!((e.amount, e.unpriced_turns, e.unpriced_models.clone()), (20.0, 2, vec!["mystery".to_string()]));
        assert_eq!(
            (groups[0].estimate, groups[0].models[0].estimate, groups[0].models[1].estimate),
            (Some(20.0), Some(20.0), None)
        );
    }

    /// The table in use belongs to one Janus: another home has none until it syncs its own.
    #[test]
    fn each_janus_reads_its_own_price_table() {
        let base = std::env::temp_dir().join(format!("janus-prices-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (one, other) =
            (crate::orchestrator::test_app(&base.join("a")), crate::orchestrator::test_app(&base.join("b")));
        let file = super::path(&one.cfg.data_dir);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, serde_json::to_vec(&Table::from_models_dev(&api(), 1)).unwrap()).unwrap();
        assert!(one.prices().is_some_and(|t| t.fetched_at == 1));
        assert!(other.prices().is_none(), "another home has no table yet");
        let _ = std::fs::remove_dir_all(&base);
    }
}
