//! `janus-skills`, the one skill through which a member's library skills reach its agent. Its
//! description is the only part the agent always sees, so it says first when to read it (at the
//! start of each task) and names as many skills, with a piece of each description, as fit. It is
//! written in the run's language. Its body is the catalog: one line per
//! skill while the catalog is small; past that, the names only, with the descriptions in
//! `catalog.txt` for the agent to search with grep, so reading the router costs about the same
//! however large the library grows. The skills most used so far come first, so they are the ones
//! kept when the description runs out of room.
// @project-doc docs/domains/20261116-resource_library.md#skill_router

use super::Item;
use crate::lang::{Lang, tr};
use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;

/// Codex rejects a skill description over 1024 bytes; the router's stays under it.
const DESCRIPTION_MAX: usize = 1000;
/// The router's body lists every skill with its description up to this many bytes (about 5k
/// tokens, the size of the agents' own skill listings: Codex gives its listing 2% of the context
/// window, Claude Code 1%). A larger library is searched in `catalog.txt` instead.
const INLINE_MAX: usize = 16 * 1024;
/// Claude Code cuts a skill's description and `when_to_use` to this many characters; the catalog
/// does the same, so one long description does not crowd out the rest.
pub(super) const ABOUT_MAX: usize = 1536;
/// Characters of each description tried in turn for the router's own description, longest first.
const SNIPPETS: [usize; 4] = [100, 50, 24, 0];
/// The descriptions of a large library, one skill per line: `<name>\t<description>`.
pub(super) const CATALOG: &str = "catalog.txt";

/// What the catalog says about one skill.
pub(super) struct Entry {
    pub name: String,
    /// The whole description, then `when_to_use` (Claude Code's second routing field), cut to
    /// what Claude Code keeps.
    pub about: String,
    /// How long `about` was before it was cut.
    pub full_len: usize,
    /// The skill says when to use it in a field of its own (`when_to_use`).
    pub when: bool,
    /// The skill asks to be used only when someone names it (`disable-model-invocation: true`).
    pub manual: bool,
    /// The skill has a `code.js`: the MCP router's `run_code` calls it as `skills.<name>(args)`.
    pub code: bool,
}

impl Entry {
    /// From a skill's SKILL.md as the agent gets it (`place_skill`).
    pub(super) fn read(item: &Item, md: &str) -> Entry {
        let field = |key| crate::front_matter::field(md, key).map(|v| one_line(&v)).filter(|v| !v.is_empty());
        let when = field("when_to_use");
        let about = [field("description").or_else(|| Some(one_line(&item.description))), when.clone()]
            .into_iter()
            .flatten()
            .filter(|v| !v.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        let manual = field("disable-model-invocation").is_some_and(|v| v == "true");
        Entry {
            name: item.name.clone(),
            full_len: about.chars().count(),
            about: crate::text::clip(&about, ABOUT_MAX - 1),
            when: when.is_some(),
            manual,
            code: false,
        }
    }

    fn line(&self, sep: &str, lang: Lang) -> String {
        let manual = if self.manual { lang.pick("（只在点名时使用）", " (only when named)") } else { "" };
        let code = if self.code {
            let key = self.name.replace('-', "_");
            tr!(
                lang,
                "（带代码：MCP 路由的 run_code 里用 skills.{key}(args) 调用）",
                " (with code: skills.{key}(args) in the MCP router's run_code)"
            )
        } else {
            String::new()
        };
        format!("{}{sep}{}{manual}{code}", self.name, self.about)
    }
}

/// Write `janus-skills/SKILL.md`, and `catalog.txt` for a large library, into `router`, whose
/// `.skills/<name>/` already holds each item's files, in `lang`. `uses` counts the sessions that
/// read each item before (`Db::skill_uses`), by item id: the most used come first, then by name.
pub(super) fn write(router: &Path, items: &[&Item], uses: &HashMap<String, u64>, lang: Lang) -> Result<()> {
    let mut entries = vec![];
    for item in items {
        let dir = router.join(".skills").join(&item.name);
        let md = std::fs::read_to_string(dir.join("SKILL.md"))?;
        let entry = Entry { code: dir.join(super::SKILL_CODE).is_file(), ..Entry::read(item, &md) };
        entries.push((uses.get(&item.id).copied().unwrap_or(0), entry));
    }
    entries.sort_by(|(a_uses, a), (b_uses, b)| b_uses.cmp(a_uses).then_with(|| a.name.cmp(&b.name)));
    let entries: Vec<Entry> = entries.into_iter().map(|(_, e)| e).collect();
    let (name, about, n) = (super::SKILL_ROUTER, description(&entries, lang), entries.len());
    let skills = router.join(".skills");
    let skills = skills.display();
    let mut body = tr!(
        lang,
        "---\nname: {name}\ndescription: {about}\n---\n\n# Janus 资源库的 Skill\n\n这里是 Janus 资源库中 {n} 个 Skill 的目录。按任务找出描述相符的 Skill，读它的 SKILL.md 再照着做，SKILL.md 提到的其他文件在它所在的目录中；没有相符的就不用。标着「只在点名时使用」的 Skill，只在用户或负责人点名时使用。\n\n每个 Skill 的 SKILL.md 在 `{skills}/<名字>/SKILL.md`。\n",
        "---\nname: {name}\ndescription: {about}\n---\n\n# Janus library skills\n\nThis is the catalog of the {n} skills in the Janus library. Find the skills whose description fits your task, read their SKILL.md and follow it; the other files a SKILL.md mentions are in its directory. If none fits, use none. Use a skill marked (only when named) only when the user or the lead names it.\n\nEach skill's SKILL.md is at `{skills}/<name>/SKILL.md`.\n"
    );
    if listed(&entries, lang) {
        let lines: Vec<String> = entries.iter().map(|e| list_line(e, lang)).collect();
        body.push_str(&format!("\n{}\n", lines.join("\n")));
    } else {
        let catalog = router.join(CATALOG);
        let text: String = entries.iter().map(|e| e.line("\t", lang) + "\n").collect();
        std::fs::write(&catalog, text)?;
        let catalog = catalog.display();
        body.push_str(&tr!(
            lang,
            "\nSkill 较多，描述不在这里，在 `{catalog}` 中，每行一个 Skill：名字、制表符、描述。先按任务想几个关键词，中文、英文和同义词都试，用 grep 查找，再读最相符的一两个 Skill 的 SKILL.md：\n\n```\ngrep -i -E '关键词1|keyword2|同义词' {catalog}\n```\n",
            "\nThere are many skills, so their descriptions are not here but in `{catalog}`, one skill per line: name, a tab, the description. Think of a few keywords for your task, in English, other languages and synonyms, search with grep, then read the SKILL.md of the one or two that fit best:\n\n```\ngrep -i -E 'keyword1|关键词2|synonym' {catalog}\n```\n"
        ));
        let names = entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>().join(lang.pick("、", ", "));
        if names.len() <= INLINE_MAX {
            body.push_str(&tr!(lang, "\n全部 Skill 的名字：{names}\n", "\nAll skill names: {names}\n"));
        }
    }
    std::fs::write(router.join("SKILL.md"), body)?;
    Ok(())
}

/// Whether the router's body lists every skill with its description; otherwise the agent
/// searches `catalog.txt`. The library shows which, judged the same way.
pub(super) fn listed<'a>(entries: impl IntoIterator<Item = &'a Entry>, lang: Lang) -> bool {
    entries.into_iter().map(|e| list_line(e, lang).len() + 1).sum::<usize>() <= INLINE_MAX
}

fn list_line(e: &Entry, lang: Lang) -> String {
    format!("- {}", e.line(lang.pick("：", ": "), lang)).replace('\n', " ")
}

/// The router's description: when to read it first, then every skill with as long a piece of its
/// description as lets them all fit; when even the names do not fit, as many names as do.
fn description(entries: &[Entry], lang: Lang) -> String {
    let n = entries.len();
    let head = tr!(
        lang,
        "每项任务开始时，先读这份目录，看 Janus 资源库的 {n} 个 Skill 中有没有适用的；有就读那个 Skill 的 SKILL.md 照着做。包括：",
        "At the start of each task, read this catalog to see whether one of the {n} skills in the Janus library fits; if one does, read its SKILL.md and follow it. Includes: "
    );
    let sep = lang.pick("、", ", ");
    for len in SNIPPETS {
        let list = entries
            .iter()
            .map(|e| {
                if len == 0 || e.about.is_empty() {
                    e.name.clone()
                } else {
                    let about = crate::text::clip(&e.about, len.saturating_sub(1));
                    tr!(lang, "{}（{about}）", "{} ({about})", e.name)
                }
            })
            .collect::<Vec<_>>()
            .join(sep);
        if head.len() + list.len() <= DESCRIPTION_MAX {
            return head + &list;
        }
    }
    let more = tr!(lang, " 等 {n} 个", ", {n} in all");
    let mut out = head;
    for (i, e) in entries.iter().enumerate() {
        let next = format!("{}{}", if i == 0 { "" } else { sep }, e.name);
        if out.len() + next.len() + more.len() > DESCRIPTION_MAX {
            break;
        }
        out.push_str(&next);
    }
    out + &more
}

/// The library skills a tool call read: each `<name>` in a `…/janus-skills/.skills/<name>/SKILL.md`
/// the call names (Claude Code's Read takes the path, Codex's shell command carries it). Reading
/// the router or searching its catalog is not reading a skill.
// @project-doc docs/domains/20261116-resource_library.md#skill_uses
pub fn skills_read(call: &str) -> Vec<String> {
    let marker = format!("/{}/.skills/", super::SKILL_ROUTER);
    let mut out: Vec<String> = vec![];
    for (at, _) in call.match_indices(&marker) {
        let rest = &call[at + marker.len()..];
        let name: String =
            rest.chars().take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-').collect();
        if !name.is_empty() && rest[name.len()..].starts_with("/SKILL.md") && !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

pub(super) fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::{Body, Origin};

    fn item(name: &str) -> Item {
        Item {
            id: name.into(),
            name: name.into(),
            description: "short".into(),
            origin: Origin::Created,
            revision: "r".into(),
            updated_at: 0,
            body: Body::Skill,
        }
    }

    /// A router over `skills`, each a (name, SKILL.md) pair, written into a fresh directory.
    fn router(tag: &str, skills: &[(String, String)]) -> std::path::PathBuf {
        router_in(tag, skills, Lang::Zh)
    }

    fn router_in(tag: &str, skills: &[(String, String)], lang: Lang) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("janus-skill-router-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (name, md) in skills {
            let d = dir.join(".skills").join(name);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("SKILL.md"), md).unwrap();
            if md.contains("with code") {
                std::fs::write(d.join(crate::library::SKILL_CODE), "export default async () => 1").unwrap();
            }
        }
        let items: Vec<Item> = skills.iter().map(|(n, _)| item(n)).collect();
        write(&dir, &items.iter().collect::<Vec<_>>(), &HashMap::new(), lang).unwrap();
        dir
    }

    fn front(text: &str) -> String {
        text.lines().find_map(|l| l.strip_prefix("description: ")).unwrap().to_string()
    }

    /// A small library is listed in the router itself, from each SKILL.md's whole description and
    /// `when_to_use`, not the library's shortened copy; a skill that asks to be named is marked.
    /// The router's description says when to open it and carries a piece of each description.
    #[test]
    fn a_small_library_is_listed_in_full() {
        let long = "x".repeat(400);
        let dir = router(
            "small",
            &[
                (
                    "pdf".into(),
                    format!(
                        "---\nname: pdf\ndescription: Fill PDF forms. {long} Use when a PDF is attached.\nwhen_to_use: The user mentions forms.\n---\nBody"
                    ),
                ),
                (
                    "deploy".into(),
                    "---\nname: deploy\ndescription: Deploy to production.\ndisable-model-invocation: true\n---\n"
                        .into(),
                ),
                ("echo-twice".into(), "---\nname: echo-twice\ndescription: Echo, with code.\n---\n".into()),
            ],
        );
        let md = std::fs::read_to_string(dir.join("SKILL.md")).unwrap();
        assert!(md.starts_with("---\nname: janus-skills\n"), "{md}");
        assert!(
            md.contains("- pdf：Fill PDF forms.")
                && md.contains("Use when a PDF is attached. The user mentions forms."),
            "{md}"
        );
        assert!(md.contains("- deploy：Deploy to production.（只在点名时使用）"), "{md}");
        assert!(
            md.contains(
                "- echo-twice：Echo, with code.（带代码：MCP 路由的 run_code 里用 skills.echo_twice(args) 调用）"
            ),
            "{md}"
        );
        assert!(md.contains(&format!("`{}/<名字>/SKILL.md`", dir.join(".skills").display())), "{md}");
        assert!(md.find("- deploy").unwrap() < md.find("- pdf").unwrap(), "by name");
        let desc = front(&md);
        assert!(desc.starts_with("每项任务开始时，先读这份目录"), "when to read it comes first: {desc}");
        assert!(desc.contains("deploy（Deploy to production.") && desc.contains("pdf（Fill PDF forms."), "{desc}");
        assert!(!dir.join(CATALOG).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A large library keeps the router about the same size: names in the router, descriptions
    /// in `catalog.txt` for grep. The description stays within what Codex takes, naming as many
    /// skills as fit.
    #[test]
    fn a_large_library_is_searched() {
        let skills: Vec<(String, String)> = (0..300)
            .map(|i| {
                let name = format!("skill-{i:03}");
                (name.clone(), format!("---\nname: {name}\ndescription: Handles case {i}. {}\n---\n", "y".repeat(200)))
            })
            .collect();
        let dir = router("large", &skills);
        let md = std::fs::read_to_string(dir.join("SKILL.md")).unwrap();
        let catalog = std::fs::read_to_string(dir.join(CATALOG)).unwrap();
        assert_eq!(catalog.lines().count(), 300);
        assert!(catalog.lines().any(|l| l.starts_with("skill-042\tHandles case 42.")), "{catalog}");
        assert!(md.contains(&format!("grep -i -E '关键词1|keyword2|同义词' {}", dir.join(CATALOG).display())), "{md}");
        assert!(md.contains("skill-000、skill-001") && !md.contains("Handles case"), "names only");
        assert!(md.len() < INLINE_MAX, "{}", md.len());
        let desc = front(&md);
        assert!(desc.len() <= DESCRIPTION_MAX && desc.ends_with(" 等 300 个"), "{desc}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The router's description gives up pieces of descriptions before it gives up names.
    #[test]
    fn the_description_keeps_names_first() {
        let entries = |n: usize| -> Vec<Entry> {
            (0..n)
                .map(|i| Entry {
                    name: format!("s{i}"),
                    about: "a".repeat(300),
                    full_len: 300,
                    when: false,
                    manual: false,
                    code: false,
                })
                .collect()
        };
        let some = description(&entries(10), Lang::Zh);
        assert!(some.len() <= DESCRIPTION_MAX, "{some}");
        assert!(some.contains(&format!("s9（{}…）", "a".repeat(49))), "shorter pieces: {some}");
        let many = description(&entries(60), Lang::Zh);
        assert!(many.len() <= DESCRIPTION_MAX && many.ends_with("s59") && !many.contains('（'), "names only: {many}");
    }

    /// A run in English gets the router in English: when to read it first in its description, the
    /// catalog's lines, the marks and the search.
    #[test]
    fn an_english_run_gets_the_router_in_english() {
        let skills = [(
            "deploy".to_string(),
            "---\nname: deploy\ndescription: Deploy to production.\ndisable-model-invocation: true\n---\n".to_string(),
        )];
        let dir = router_in("english", &skills, Lang::En);
        let md = std::fs::read_to_string(dir.join("SKILL.md")).unwrap();
        let desc = front(&md);
        assert!(desc.starts_with("At the start of each task, read this catalog"), "{desc}");
        assert!(desc.contains("deploy (Deploy to production."), "{desc}");
        assert!(md.contains("- deploy: Deploy to production. (only when named)"), "{md}");
        assert!(md.contains(&format!("`{}/<name>/SKILL.md`", dir.join(".skills").display())), "{md}");
        assert!(!md.contains("Skill 的目录"), "{md}");
        let _ = std::fs::remove_dir_all(&dir);
        let many: Vec<(String, String)> = (0..300)
            .map(|i| (format!("s-{i:03}"), format!("---\nname: s-{i:03}\ndescription: {}\n---\n", "y".repeat(200))))
            .collect();
        let dir = router_in("english-large", &many, Lang::En);
        let md = std::fs::read_to_string(dir.join("SKILL.md")).unwrap();
        assert!(md.contains("grep -i -E 'keyword1|") && md.contains("All skill names: s-000, s-001"), "{md}");
        assert!(front(&md).ends_with(", 300 in all"), "{}", front(&md));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The skills used most come first, in the catalog and in the description.
    #[test]
    fn the_most_used_come_first() {
        let dir = std::env::temp_dir().join(format!("janus-skill-router-uses-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let items: Vec<Item> = ["alpha", "beta", "gamma"].map(item).into_iter().collect();
        for i in &items {
            let d = dir.join(".skills").join(&i.name);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("SKILL.md"), format!("---\nname: {}\ndescription: Does {}.\n---\n", i.name, i.name))
                .unwrap();
        }
        let uses = HashMap::from([("gamma".to_string(), 5), ("beta".to_string(), 2)]);
        write(&dir, &items.iter().collect::<Vec<_>>(), &uses, Lang::Zh).unwrap();
        let md = std::fs::read_to_string(dir.join("SKILL.md")).unwrap();
        let at = |s: &str| md.rfind(s).unwrap();
        assert!(at("- gamma") < at("- beta") && at("- beta") < at("- alpha"), "{md}");
        assert!(front(&md).contains("gamma（Does gamma.）、beta（Does beta.）、alpha"), "{md}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A tool call that reads a skill's SKILL.md names it, whichever agent made it; reading the
    /// router or grepping its catalog does not.
    #[test]
    fn reading_a_skill_is_seen_in_the_call() {
        let base = "/j/runs/r/members/m1/bundle/skills/janus-skills";
        let claude = format!(r#"{{"file_path":"{base}/.skills/pdf-forms/SKILL.md"}}"#);
        assert_eq!(skills_read(&claude), ["pdf-forms"]);
        let codex = format!(
            r#"{{"command":["bash","-lc","sed -n '1,200p' {base}/.skills/notes/SKILL.md && cat {base}/.skills/notes/SKILL.md"]}}"#
        );
        assert_eq!(skills_read(&codex), ["notes"], "once per call");
        assert!(skills_read(&format!("cat {base}/SKILL.md; grep -i pdf {base}/catalog.txt")).is_empty());
        assert!(
            skills_read(&format!("ls {base}/.skills/pdf-forms/scripts/")).is_empty(),
            "a skill's other files are not its SKILL.md"
        );
    }
}
