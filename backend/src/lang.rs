//! The language Janus speaks to agents in: the language the person uses the app in. The app tells
//! the service (`PUT /api/language`), which keeps it as a setting; each run takes it when it is
//! created and keeps it, so a run's sessions never change language halfway.
// @project-doc docs/domains/20261031-goal_mode.md#prompt_language

use crate::db::ConfigKind;
use crate::orchestrator::App;
use anyhow::Result;
use serde_json::{Value, json};

crate::wire::wire_enum! {
    /// A language Janus writes what it tells agents in.
    #[derive(Default)]
    pub enum Lang {
        #[default]
        Zh = "zh",
        En = "en",
    }
}

impl Lang {
    /// The text in this language, from the two versions.
    pub fn pick<'a>(self, zh: &'a str, en: &'a str) -> &'a str {
        match self {
            Lang::Zh => zh,
            Lang::En => en,
        }
    }

    /// A run's language as its snapshot records it.
    pub fn of_config(config: &Value) -> Lang {
        config["lang"].as_str().and_then(Lang::parse).unwrap_or_default()
    }
}

/// Format the text in `lang`'s version: `tr!(lang, "中文 {x}", "English {x}")`, with the same
/// arguments for both, written inline or after the two strings.
macro_rules! tr {
    ($lang:expr, $zh:literal, $en:literal $(,)?) => {
        match $lang {
            $crate::lang::Lang::Zh => format!($zh),
            $crate::lang::Lang::En => format!($en),
        }
    };
    ($lang:expr, $zh:literal, $en:literal, $($arg:tt)+) => {
        match $lang {
            $crate::lang::Lang::Zh => format!($zh, $($arg)+),
            $crate::lang::Lang::En => format!($en, $($arg)+),
        }
    };
}
pub(crate) use tr;

/// The language the app is used in: the one row of `ConfigKind::Language`.
#[derive(serde::Serialize, serde::Deserialize)]
struct AppLanguage {
    language: Lang,
}

impl crate::db::Setting for AppLanguage {
    const KIND: ConfigKind = ConfigKind::Language;
    fn id(&self) -> &str {
        "app"
    }
}

impl App {
    /// The language the person last used the app in; Chinese until the app says.
    pub fn language(&self) -> Lang {
        self.db.get::<AppLanguage>("app").ok().flatten().map(|s| s.language).unwrap_or_default()
    }

    /// The language run `run_id` was created in, from its record.
    pub fn run_lang(&self, run_id: &str) -> Lang {
        self.db.run(run_id).ok().flatten().map(|r| Lang::of_config(&r.config)).unwrap_or_default()
    }

    /// Keep the app's language for the runs created from now on.
    pub fn save_language(&self, body: &Value) -> Result<Value> {
        let lang = body["language"]
            .as_str()
            .and_then(Lang::parse)
            .ok_or_else(|| anyhow::anyhow!("language 只能是 zh 或 en"))?;
        self.db.put(&AppLanguage { language: lang }, crate::util::now_ms())?;
        Ok(json!({ "language": lang }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tr_picks_the_version_and_captures_names() {
        let n = 3;
        assert_eq!(tr!(Lang::Zh, "有 {n} 条", "{n} notes"), "有 3 条");
        assert_eq!(tr!(Lang::En, "有 {n} 条", "{n} notes"), "3 notes");
        assert_eq!(tr!(Lang::En, "有 {} 条", "{} notes", n + 1), "4 notes");
        assert_eq!(Lang::of_config(&json!({ "lang": "en" })), Lang::En);
    }
}
