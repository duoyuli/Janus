//! Hints on how findable each library skill is. An agent picks a skill from the router's catalog
//! by its description alone, so the library points out descriptions that are too short to route
//! on, that never say when to use the skill, that the catalog has to cut, or that read so much
//! like another skill's that the agent may not tell them apart. They are hints: nothing is
//! refused, and the skill reaches every session as it is.
// @project-doc docs/domains/20261116-resource_library.md#skill_hints

use super::skill_router::{ABOUT_MAX, Entry};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

/// One thing that keeps a skill from being found; the app words it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Hint {
    /// Too short to route on: say what the skill does and when to use it.
    Short,
    /// Never says when to use the skill, in the description or `when_to_use`.
    NoWhen,
    /// Longer than the catalog keeps (`chars` against `kept`): put the key use first.
    Long { chars: usize, kept: usize },
    /// Reads so much like `skill`'s description that the agent may not tell them apart.
    Alike { skill: String },
}

/// A description shorter than this says too little to route on. A Chinese character counts as
/// two, as it carries about as much as a short English word does.
const SHORT: usize = 40;
/// Two descriptions sharing this share of their words (Jaccard) read alike.
const ALIKE: f64 = 0.6;
/// Descriptions with fewer distinct words than this are not compared: a short one is flagged
/// as short already.
const COMPARABLE: usize = 6;

/// Words that say when to use something, in English and Chinese.
const WHEN: [&str; 14] = [
    "when",
    "use for",
    "used for",
    "use if",
    "trigger",
    "invoke",
    "用于",
    "时使用",
    "时用",
    "时调用",
    "适用",
    "适合",
    "当",
    "如果",
];

/// Common words that do not tell skills apart.
const STOP: [&str; 14] =
    ["the", "and", "for", "use", "when", "with", "this", "that", "from", "into", "your", "you", "are", "skill"];

/// The hints for each skill, by item id; a skill with nothing to point out has none.
pub(super) fn skill_hints(skills: &[(String, Entry)]) -> HashMap<String, Vec<Hint>> {
    let words: Vec<HashSet<String>> = skills.iter().map(|(_, e)| words(&e.about)).collect();
    let mut out: HashMap<String, Vec<Hint>> = HashMap::new();
    for (i, (id, e)) in skills.iter().enumerate() {
        let mut hints = vec![];
        if weight(&e.about) < SHORT {
            hints.push(Hint::Short);
        } else if !e.when && !says_when(&e.about) {
            hints.push(Hint::NoWhen);
        }
        if e.full_len > ABOUT_MAX {
            hints.push(Hint::Long { chars: e.full_len, kept: ABOUT_MAX });
        }
        for (j, (_, other)) in skills.iter().enumerate() {
            if i != j && alike(&words[i], &words[j]) {
                hints.push(Hint::Alike { skill: other.name.clone() });
            }
        }
        if !hints.is_empty() {
            out.insert(id.clone(), hints);
        }
    }
    out
}

fn weight(about: &str) -> usize {
    about.chars().map(|c| if cjk(c) { 2 } else { 1 }).sum()
}

fn cjk(c: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&c)
}

fn says_when(about: &str) -> bool {
    let about = about.to_lowercase();
    WHEN.iter().any(|w| about.contains(w))
}

/// The words of a description: English words of three letters or more, and pairs of adjacent
/// Chinese characters (Chinese has no spaces between words).
fn words(about: &str) -> HashSet<String> {
    let about = about.to_lowercase();
    let mut out = HashSet::new();
    for w in about.split(|c: char| !c.is_ascii_alphanumeric()) {
        if w.len() >= 3 && !STOP.contains(&w) {
            out.insert(w.to_string());
        }
    }
    let chars: Vec<char> = about.chars().collect();
    for pair in chars.windows(2) {
        if pair.iter().all(|c| cjk(*c)) {
            out.insert(pair.iter().collect());
        }
    }
    out
}

fn alike(a: &HashSet<String>, b: &HashSet<String>) -> bool {
    if a.len() < COMPARABLE || b.len() < COMPARABLE {
        return false;
    }
    a.intersection(b).count() as f64 / a.union(b).count() as f64 >= ALIKE
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, about: &str, when: bool) -> (String, Entry) {
        let e = Entry {
            name: name.into(),
            about: about.into(),
            full_len: about.chars().count(),
            when,
            manual: false,
            code: false,
        };
        (format!("id-{name}"), e)
    }

    /// A description good to route on gets no hint; a short one, one that never says when, one
    /// the catalog cuts, and two that read alike each get theirs.
    #[test]
    fn hints_point_at_what_keeps_a_skill_from_being_found() {
        let good = "Extract text and tables from PDF files and fill forms. Use when the user attaches a PDF.";
        let mut long = entry(
            "long",
            &format!(
                "Builds release notes from merged pull requests. Use when cutting a release. {}",
                "x ".repeat(800)
            ),
            false,
        );
        long.1.full_len = 1700;
        let skills = vec![
            entry("pdf", good, false),
            entry("short", "写周报", false),
            entry("nowhen", "Formats SQL queries consistently across dialects and keeps comments", false),
            entry("field", "Formats Swift files consistently with the house style", true),
            entry("zh", "在需要生成数据库迁移脚本时使用，按项目约定写好升级和回滚", false),
            long,
            entry(
                "review-a",
                "Review a pull request for bugs, style problems and missing tests. Use when asked to review",
                false,
            ),
            entry(
                "review-b",
                "Review a pull request for bugs, style problems and missing tests. Use when asked for review",
                false,
            ),
        ];
        let hints = skill_hints(&skills);
        assert!(
            !hints.contains_key("id-pdf") && !hints.contains_key("id-field") && !hints.contains_key("id-zh"),
            "{hints:?}"
        );
        assert_eq!(hints["id-short"], [Hint::Short]);
        assert_eq!(hints["id-nowhen"], [Hint::NoWhen]);
        assert_eq!(hints["id-long"], [Hint::Long { chars: 1700, kept: ABOUT_MAX }]);
        assert_eq!(hints["id-review-a"], [Hint::Alike { skill: "review-b".into() }]);
        assert_eq!(hints["id-review-b"], [Hint::Alike { skill: "review-a".into() }]);
        let json = serde_json::to_value(&hints["id-long"]).unwrap();
        assert_eq!(json, serde_json::json!([{ "kind": "long", "chars": 1700, "kept": 1536 }]));
    }

    #[test]
    fn chinese_descriptions_are_compared_by_character_pairs() {
        let a = words("在需要生成数据库迁移脚本时使用");
        let b = words("在需要生成数据库迁移脚本的时候使用");
        assert!(a.contains("迁移") && alike(&a, &b), "{a:?} {b:?}");
        assert!(!alike(&a, &words("把会议录音整理成纪要，标出待办和负责人")));
    }
}
