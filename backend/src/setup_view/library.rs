//! The resource library in words: where each item came from, where an MCP server is, how many of
//! its values are set, how often a skill was read, the hints on its description, what keeps it
//! from working; and each GitHub source's name, its line and the skills not imported from it yet.
// @project-doc docs/architecture/20261031-frontend.md#setup_lines

use super::s;
use crate::lang::{Lang, tr};
use serde_json::{Value, json};

/// Where an item came from, in a few words.
fn origin(item: &Value, lang: Lang) -> String {
    let o = &item["origin"];
    match s(&o["type"]) {
        "folder" => {
            let path = s(&o["path"]);
            tr!(lang, "复制自 {path}", "Copied from {path}")
        }
        "git" => {
            let from = s(&item["source"]["url"]).replace("https://github.com/", "");
            if item["source"]["inSource"] == true {
                tr!(lang, "来自 {from}", "From {from}")
            } else {
                tr!(lang, "来自 {from} · 来源中已删除", "From {from} · Removed upstream")
            }
        }
        _ if item["kind"] == "skill" => lang.pick("在 Janus 中新建", "Created in Janus").into(),
        _ => lang.pick("手动添加", "Added Manually").into(),
    }
}

/// Where an MCP server is: "stdio · npx -y gh-mcp", "HTTP · https://…", "SSE · https://…".
fn endpoint(item: &Value) -> String {
    let srv = &item["server"];
    match s(&srv["transport"]) {
        "http" => format!("HTTP · {}", s(&srv["url"])),
        "sse" => format!("SSE · {}", s(&srv["url"])),
        _ => {
            let mut words = vec![s(&srv["command"]).to_string()];
            words.extend(srv["args"].as_array().into_iter().flatten().map(|a| s(a).to_string()));
            format!("stdio · {}", words.join(" "))
        }
    }
}

/// How many of a server's variables have values, e.g. "变量 1/2 已设置", and whether all do; empty
/// when it has none.
fn vars(item: &Value, lang: Lang) -> (String, bool) {
    let vars = item["vars"].as_array().map(Vec::as_slice).unwrap_or_default();
    if vars.is_empty() {
        return (String::new(), true);
    }
    let set = vars.iter().filter(|v| v["set"] == true).count();
    let n = vars.len();
    let what = if item["server"]["transport"] == "stdio" { lang.pick("变量", "Variables") } else { "Header" };
    (tr!(lang, "{what} {set}/{n} 已设置", "{what}: {set} of {n} set"), set == n)
}

/// How much a skill has been read, without when last: the client writes that time its own way.
fn usage(item: &Value, lang: Lang) -> String {
    match item["uses"]["sessions"].as_i64().unwrap_or(0) {
        0 => lang.pick("还没有会话读过", "Not read by any session yet").into(),
        n => tr!(lang, "被 {n} 个会话读过", "Sessions that read it: {n}"),
    }
}

/// A count with thousands separated, as English writes it: 1,700.
fn grouped(n: i64) -> String {
    let digits = n.abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}

/// One hint on how findable a skill is, in words.
fn hint(h: &Value, lang: Lang) -> String {
    match s(&h["kind"]) {
        "short" => lang
            .pick(
                "描述太短：Agent 只凭描述判断什么时候用它，写清它做什么、什么时候用",
                "The description is too short: the agent decides when to use a skill from its description alone, so say what it does and when to use it.",
            )
            .into(),
        "noWhen" => lang
            .pick(
                "描述没写什么时候用：加一句「在……时使用」，或写在 when_to_use 字段里",
                "The description never says when to use the skill: add “Use when …”, or put it in the when_to_use field.",
            )
            .into(),
        "long" => {
            let n = |k: &str| h[k].as_i64().unwrap_or(0);
            let (chars, kept) = match lang {
                Lang::Zh => (n("chars").to_string(), n("kept").to_string()),
                Lang::En => (grouped(n("chars")), grouped(n("kept"))),
            };
            tr!(
                lang,
                "描述有 {chars} 个字，目录里只保留前 {kept} 个：把关键用途和触发词写在开头",
                "The description has {chars} characters; the catalog keeps the first {kept}, so put the key use and trigger words first."
            )
        }
        "alike" => {
            let other = s(&h["skill"]);
            tr!(
                lang,
                "和 {other} 的描述很像，Agent 可能分不清用哪个：写出各自适用的场合",
                "Reads much like {other}'s description, so the agent may not tell them apart: say when each one applies."
            )
        }
        _ => String::new(),
    }
}

/// Add what the library shows of an item: `originText`, `endpoint` (a server), `varsText` and
/// `varsComplete`, `usageText` (a skill), `hintTexts` and `warning`, the one line of what keeps
/// it from working (values not filled in, gone from its source, a description to improve).
pub fn dress_item(item: &mut Value, lang: Lang) {
    let mcp = item["kind"] == "mcp";
    item["originText"] = json!(origin(item, lang));
    if mcp {
        item["endpoint"] = json!(endpoint(item));
    } else {
        item["usageText"] = json!(usage(item, lang));
    }
    let (vars_text, complete) = vars(item, lang);
    let hints: Vec<String> = item["hints"].as_array().into_iter().flatten().map(|h| hint(h, lang)).collect();
    let gone = item["origin"]["type"] == "git" && item["source"]["inSource"] != true;
    let mut warning = vec![];
    if !complete {
        warning.push(vars_text.clone());
    }
    if gone {
        warning.push(lang.pick("来源中已删除", "Removed upstream").to_string());
    }
    if !hints.is_empty() {
        warning.push(lang.pick("描述可以改进", "Description could be better").to_string());
    }
    item["varsText"] = json!(vars_text);
    item["varsComplete"] = json!(complete);
    item["hintTexts"] = json!(hints);
    item["warning"] = json!(warning.join(" · "));
}

/// Add what the library shows of a GitHub source: its `name` (`owner/repo`, with the folder and
/// branch its link names), the skills it offers not imported yet (`pending`) and its `caption`.
pub fn dress_source(src: &mut Value, lang: Lang) {
    let (path, git_ref) = (s(&src["path"]), s(&src["gitRef"]));
    let mut name = s(&src["repo"]).to_string();
    if !path.is_empty() {
        name = format!("{name}/{path}");
    }
    if !git_ref.is_empty() {
        name = format!("{name} @{git_ref}");
    }
    let imported: Vec<&str> = src["imported"].as_array().into_iter().flatten().map(|i| s(&i["path"])).collect();
    let pending: Vec<Value> = src["offers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|o| !imported.contains(&s(&o["path"])))
        .cloned()
        .collect();
    let commit: String = s(&src["commit"]).chars().take(7).collect();
    let mut caption = tr!(lang, "GitHub · 提交 {commit}", "GitHub · Commit {commit}");
    if !pending.is_empty() {
        let n = pending.len();
        caption += &tr!(lang, " · {n} 个未导入", " · {n} not imported");
    }
    src["name"] = json!(name);
    src["caption"] = json!(caption);
    src["pending"] = json!(pending);
}

/// The library as `GET /api/library` returns it, in words for `lang`.
pub fn dress_library(lib: &mut Value, lang: Lang) {
    for i in lib["items"].as_array_mut().into_iter().flatten() {
        dress_item(i, lang);
    }
    for src in lib["sources"].as_array_mut().into_iter().flatten() {
        dress_source(src, lang);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::Lang::{En, Zh};

    #[test]
    fn an_item_says_where_it_came_from_and_what_keeps_it_from_working() {
        let mut gone = json!({
            "kind": "skill", "origin": { "type": "git" }, "source": { "url": "https://github.com/o/r", "inSource": false },
            "uses": { "sessions": 3 }, "hints": [{ "kind": "short" }, { "kind": "long", "chars": 900, "kept": 300 }],
        });
        dress_item(&mut gone, Zh);
        assert_eq!(gone["originText"], "来自 o/r · 来源中已删除");
        assert_eq!(gone["usageText"], "被 3 个会话读过");
        assert_eq!(gone["hintTexts"][1], "描述有 900 个字，目录里只保留前 300 个：把关键用途和触发词写在开头");
        assert_eq!(gone["warning"], "来源中已删除 · 描述可以改进");
        assert!(gone.get("endpoint").is_none());

        let mut server = json!({
            "kind": "mcp", "origin": {}, "server": { "transport": "stdio", "command": "npx", "args": ["-y", "gh-mcp"] },
            "vars": [{ "name": "A", "set": true }, { "name": "B", "set": false }],
        });
        dress_item(&mut server, En);
        assert_eq!(server["originText"], "Added Manually");
        assert_eq!(server["endpoint"], "stdio · npx -y gh-mcp");
        assert_eq!(
            (server["varsText"].as_str(), server["varsComplete"].as_bool()),
            (Some("Variables: 1 of 2 set"), Some(false))
        );
        assert_eq!(server["warning"], "Variables: 1 of 2 set");

        let mut http = json!({ "kind": "mcp", "origin": { "type": "folder", "path": "~/x" }, "server": { "transport": "http", "url": "https://h" }, "vars": [{ "set": true }] });
        dress_item(&mut http, Zh);
        assert_eq!(http["endpoint"], "HTTP · https://h");
        assert_eq!(http["varsText"], "Header 1/1 已设置");
        assert_eq!(http["warning"], "");
        assert_eq!(http["originText"], "复制自 ~/x");
        assert_eq!(
            hint(&json!({ "kind": "long", "chars": 1700, "kept": 1536 }), En),
            "The description has 1,700 characters; the catalog keeps the first 1,536, so put the key use and trigger words first."
        );
        assert_eq!(
            hint(&json!({ "kind": "alike", "skill": "review" }), Zh),
            "和 review 的描述很像，Agent 可能分不清用哪个：写出各自适用的场合"
        );
        assert!(
            !hint(&json!({ "kind": "short" }), En).is_empty() && !hint(&json!({ "kind": "noWhen" }), Zh).is_empty()
        );
        let mut fresh = json!({ "kind": "skill", "origin": { "type": "created" } });
        dress_item(&mut fresh, Zh);
        assert_eq!(
            (fresh["originText"].as_str(), fresh["usageText"].as_str()),
            (Some("在 Janus 中新建"), Some("还没有会话读过"))
        );
    }

    #[test]
    fn a_source_names_its_repository_and_the_skills_still_to_import() {
        let mut src = json!({
            "repo": "o/r", "path": "skills", "gitRef": "dev", "commit": "0123456789",
            "offers": [{ "path": "a" }, { "path": "b" }], "imported": [{ "path": "a", "name": "a" }],
        });
        dress_source(&mut src, Zh);
        assert_eq!(src["name"], "o/r/skills @dev");
        assert_eq!(src["caption"], "GitHub · 提交 0123456 · 1 个未导入");
        assert_eq!(src["pending"], json!([{ "path": "b" }]));
        let mut all =
            json!({ "repo": "o/r", "commit": "abc", "offers": [{ "path": "a" }], "imported": [{ "path": "a" }] });
        dress_source(&mut all, En);
        assert_eq!((all["name"].as_str(), all["caption"].as_str()), (Some("o/r"), Some("GitHub · Commit abc")));
    }
}
