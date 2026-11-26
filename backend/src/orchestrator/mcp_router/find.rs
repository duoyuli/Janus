//! `find_tools` and how `call_tool` and `run_code` find a tool by name, after magpie's MCP router
//! (`hub_ondemand.go`). The agent sees none of the servers' tools until it looks: `find_tools`
//! matches a query's key words against each tool's routed name, its server's name and its
//! description, and gives the best few whole; `call_tool` takes the routed name, or a name close
//! enough to mean one tool.
// @project-doc docs/domains/20261116-resource_library.md#mcp_router

use super::Routed;
use super::say::{SAY_MAX, tool_clause};
use crate::lang::{Lang, tr};
use serde_json::{Value, json};

/// How many matches come with their parameters, and how many more are named after them.
const FULL: usize = 5;
const MORE: usize = 15;
/// How many of each server's tools are named when naming them all would be too long.
const SERVERS_SAID: usize = 8;

/// The tool of `find_tools`.
pub(super) const FIND: &str = "find_tools";
/// The tool of `call_tool`.
pub(super) const CALL: &str = "call_tool";

/// Whether `c` is of a script written without spaces between words: Chinese, Japanese, Korean.
fn spaceless(c: char) -> bool {
    han(c)
        || matches!(c as u32, 0x3040..=0x30FF | 0x31F0..=0x31FF | 0xAC00..=0xD7AF | 0x1100..=0x11FF | 0x3130..=0x318F)
}

fn han(c: char) -> bool {
    matches!(c as u32, 0x4E00..=0x9FFF | 0x3400..=0x4DBF | 0x20000..=0x2FFFF | 0xF900..=0xFAFF)
}

/// Whether `s` has a letter outside ASCII (an em dash or a curly quote is none).
fn non_ascii(s: &str) -> bool {
    s.chars().any(|c| !c.is_ascii() && c.is_alphabetic())
}

/// `s`'s words, lower case, split at anything not a letter or digit (`create_issue` is `create` and
/// `issue`) and where a script without spaces meets one with them (`截图screenshot`).
fn words(s: &str) -> Vec<String> {
    let mut out = vec![];
    let mut w = String::new();
    let mut was_spaceless = false;
    for c in s.to_lowercase().chars() {
        if !c.is_alphanumeric() {
            if !w.is_empty() {
                out.push(std::mem::take(&mut w));
            }
            continue;
        }
        if !w.is_empty() && spaceless(c) != was_spaceless {
            out.push(std::mem::take(&mut w));
        }
        was_spaceless = spaceless(c);
        w.push(c);
    }
    if !w.is_empty() {
        out.push(w);
    }
    out
}

/// Words of a query that say nothing of what a tool does.
const STOP: &[&str] = &[
    "a", "an", "the", "of", "to", "for", "in", "on", "at", "by", "with", "and", "or", "from", "into", "is", "are",
    "be", "it", "its", "this", "that", "my", "me", "i", "you", "your", "some", "please", "tool", "tools", "can", "do",
    "how", "what",
];

/// Characters of a query without spaces that say nothing of a tool, where a word is cut (帮我截个图
/// is 截 and 图): pronouns, particles, measure words. Only those that seldom start or end a word a
/// tool is described with.
const STOP_RUNES: &str = "我你他她它们的了吗呢吧啊呀么个帮把给这那一をはがのにでとへも";

/// A word without spaces cut at `STOP_RUNES`, each piece one key word; a lone character that is
/// not Han says nothing.
fn spaceless_keys(w: &str) -> Vec<String> {
    w.split(|c| STOP_RUNES.contains(c))
        .filter(|p| !p.is_empty())
        .filter(|p| p.chars().count() > 1 || p.chars().next().is_some_and(han))
        .map(str::to_string)
        .collect()
}

/// Whether `w`, a key word without spaces, is in `text`: a character is; a longer one has at least
/// half of its two-character runs there (屏幕截图 and 截取屏幕 share 屏幕).
fn phrase_in(w: &str, text: &str) -> bool {
    let cs: Vec<char> = w.chars().collect();
    if cs.len() == 1 {
        return text.contains(w);
    }
    let found = cs.windows(2).filter(|p| text.contains(&p.iter().collect::<String>())).count();
    found * 2 >= cs.len() - 1
}

/// The query's words that say what is wanted, each once.
fn key_words(query: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    let mut add = |w: String| {
        if !out.contains(&w) {
            out.push(w);
        }
    };
    for w in words(query) {
        if w.chars().next().is_some_and(spaceless) {
            spaceless_keys(&w).into_iter().for_each(&mut add);
        } else if !STOP.contains(&w.as_str()) && w.chars().count() >= 2 {
            add(w);
        }
    }
    out
}

/// Whether `w` is one of `ws`, or (four letters or more) the start of one (`repo` is
/// `repository`'s), or one of four letters or more is `w`'s start (`issues` has `issue`). A word
/// without spaces is as `phrase_in` finds it; another outside ASCII anywhere in `text`.
fn word_in(w: &str, ws: &[String], text: &str) -> bool {
    if w.chars().next().is_some_and(spaceless) {
        return phrase_in(w, text);
    }
    if non_ascii(w) {
        return text.contains(w);
    }
    ws.iter().any(|x| x == w || (w.len() >= 4 && x.starts_with(w)) || (x.len() >= 4 && w.starts_with(x.as_str())))
}

/// How well one tool matches a query: key words in its names, and in its description only, of the
/// key words that count for it; `exact` is the query being its name.
struct Match<'a> {
    tool: &'a Routed,
    names: usize,
    descs: usize,
    keys: usize,
    exact: bool,
}

impl Match<'_> {
    /// Whether it matches at all: it is the query's name, or it has two thirds of the key words,
    /// one of them in its name unless it has them all. A word or two of a longer ask in a
    /// description is not what was asked for, and a tool of a near name that does something else
    /// must not be handed over.
    fn clears(&self) -> bool {
        let got = self.names + self.descs;
        self.exact || (self.keys > 0 && got * 3 >= self.keys * 2 && (self.names > 0 || got == self.keys))
    }

    fn score(&self) -> usize {
        3 * self.names + self.descs + if self.exact { 100 } else { 0 }
    }
}

/// A tool's description as its server gives it.
fn description(t: &Routed) -> &str {
    t.spec["description"].as_str().unwrap_or_default()
}

/// How `t` matches `query`, whose key words are `keys`. A key word outside ASCII counts only for a
/// tool that has such letters: a query of 截图 screenshot is screenshot to a tool described in
/// English.
fn match_of<'a>(t: &'a Routed, query: &str, keys: &[String]) -> Match<'a> {
    let q = query.trim();
    let exact = !q.is_empty() && (t.name.eq_ignore_ascii_case(q) || t.tool.eq_ignore_ascii_case(q));
    let name = format!("{} {}", t.name, t.item.name).to_lowercase();
    let desc = description(t).to_lowercase();
    let (nw, dw) = (words(&name), words(&desc));
    let own = non_ascii(&name) || non_ascii(&desc);
    let mut m = Match { tool: t, names: 0, descs: 0, keys: 0, exact };
    for w in keys {
        if !own && non_ascii(w) {
            continue;
        }
        m.keys += 1;
        if word_in(w, &nw, &name) {
            m.names += 1;
        } else if word_in(w, &dw, &desc) {
            m.descs += 1;
        }
    }
    m
}

/// The tools that match `query`, best first: none when no tool does what it asks.
fn matches<'a>(tools: &'a [Routed], query: &str) -> Vec<&'a Routed> {
    let keys = key_words(query);
    let mut ms: Vec<Match> = tools.iter().map(|t| match_of(t, query, &keys)).filter(Match::clears).collect();
    ms.sort_by_key(|m| std::cmp::Reverse(m.score()));
    ms.into_iter().map(|m| m.tool).collect()
}

/// The tools whose names share a word with `name`, most first: what a mistyped name meant.
pub(super) fn near<'a>(tools: &'a [Routed], name: &str) -> Vec<&'a Routed> {
    let keys = key_words(name);
    let mut ms: Vec<Match> = tools
        .iter()
        .map(|t| match_of(t, name, &keys))
        .filter(|m| m.names > 0)
        .map(|m| Match { descs: 0, ..m })
        .collect();
    ms.sort_by_key(|m| std::cmp::Reverse(m.score()));
    ms.into_iter().map(|m| m.tool).collect()
}

/// The tools, one a line, each with what it does in a few words.
fn name_lines(tools: &[&Routed]) -> String {
    tools
        .iter()
        .map(|t| {
            let c = tool_clause(description(t));
            if c.is_empty() { format!("- {}\n", t.name) } else { format!("- {}: {c}\n", t.name) }
        })
        .collect()
}

/// Every server in order, each with how many tools it has and their names: what there is, said as
/// what there is and not as matches. Every tool when that fits `SAY_MAX`, else the first
/// `SERVERS_SAID` of each server's, and `all` is false.
fn server_counts(tools: &[Routed]) -> (String, bool) {
    let mut servers: Vec<(&str, Vec<&str>)> = vec![];
    for t in tools {
        match servers.iter_mut().find(|(s, _)| *s == t.item.name) {
            Some((_, names)) => names.push(&t.tool),
            None => servers.push((&t.item.name, vec![&t.tool])),
        }
    }
    let say = |most: usize| {
        let mut all = true;
        let parts: Vec<String> = servers
            .iter()
            .map(|(s, names)| {
                let mut said = names[..names.len().min(most)].join(", ");
                if names.len() > most {
                    said += ", …";
                    all = false;
                }
                format!("{s} ({}: {said})", names.len())
            })
            .collect();
        (parts.join("; "), all)
    };
    let (said, all) = say(usize::MAX);
    if said.chars().count() <= SAY_MAX { (said, all) } else { say(SERVERS_SAID) }
}

/// The answer when nothing matches: that no tool was found and not to call one that only sounds
/// close, every server with its tools, and how to look again.
fn not_found(tools: &[Routed], query: &str, lang: Lang) -> String {
    let (said, all) = server_counts(tools);
    let mut out = tr!(
        lang,
        "没有找到和「{query}」相符的工具：没有工具的名字或说明有它要的。不要调用只是名字相近的工具。现有的服务器和工具（括号里是数量）：{said}。",
        "No tool was found for \"{query}\": no tool's name or description has what it asks. Don't call a tool that only sounds close. The servers and their tools (how many in brackets): {said}. "
    );
    if !all {
        out +=
            &tr!(lang, "query 为空的 find_tools 列出所有工具。", "find_tools with an empty query names every tool. ");
    }
    let described_in_english =
        !tools.iter().any(|t| non_ascii(&format!("{} {} {}", t.name, t.item.name, description(t))));
    if non_ascii(query) && described_in_english {
        out += &tr!(
            lang,
            "这些工具的名字和说明都是英文的：用英文词再找一次。",
            "The tools are named and described in English: find_tools with English words looks again."
        );
    } else {
        out += &tr!(
            lang,
            "换个说法、服务器名或工具名再找一次。",
            "find_tools with other words, a server or a tool's name looks again."
        );
    }
    out
}

/// The answer to `find_tools`: the best matches with their parameters, then more by name; with no
/// query every tool by name; with nothing matching, the servers and their tools.
pub(super) fn find(tools: &[Routed], args: &Value, lang: Lang) -> (String, bool) {
    let query = args["query"].as_str().unwrap_or_default();
    let server = args["server"].as_str().unwrap_or_default().trim();
    let tools: Vec<Routed> =
        tools.iter().filter(|t| server.is_empty() || t.item.name.eq_ignore_ascii_case(server)).cloned().collect();
    if tools.is_empty() {
        return if server.is_empty() {
            (
                tr!(lang, "这个服务器背后的 MCP 服务器现在没有工具。", "No MCP server behind this one has a tool now."),
                true,
            )
        } else {
            (
                tr!(
                    lang,
                    "这个服务器背后没有叫 {server} 的服务器。不带 server 调用 find_tools 看所有工具。",
                    "No server called {server} is behind this one. Call find_tools without server for every tool."
                ),
                true,
            )
        };
    }
    if query.trim().is_empty() {
        let all: Vec<&Routed> = tools.iter().collect();
        let head = tr!(
            lang,
            "所有工具的名字；用名字调用 find_tools 看它的参数：",
            "Every tool, by name; find_tools with its name gives its parameters:"
        );
        return (format!("{head}\n{}", name_lines(&all)), false);
    }
    let ms = matches(&tools, query);
    if ms.is_empty() {
        return (not_found(&tools, query, lang), false);
    }
    let full: Vec<Value> = ms
        .iter()
        .take(FULL)
        .map(|t| {
            json!({
                "name": t.name,
                "description": description(t),
                "inputSchema": t.spec.get("inputSchema").cloned().unwrap_or_else(|| json!({ "type": "object", "properties": {} })),
            })
        })
        .collect();
    let mut out = format!(
        "{}\n{}",
        tr!(
            lang,
            "用 call_tool 调用，名字和参数按它的 inputSchema：",
            "Run one with call_tool, its name and arguments as its inputSchema says."
        ),
        serde_json::to_string_pretty(&full).unwrap_or_default()
    );
    let more: Vec<&Routed> = ms.iter().skip(FULL).take(MORE).copied().collect();
    if !more.is_empty() {
        let head = tr!(
            lang,
            "也相符的（用名字调用 find_tools 看参数）：",
            "Also matching (find_tools with a name gives its parameters):"
        );
        out += &format!("\n{head}\n{}", name_lines(&more));
    }
    (out, false)
}

/// `call_tool`'s arguments: the tool's name and its own arguments, an object; given as a string
/// holding one, as some models send it, the object it holds.
pub(super) fn call_args(args: &Value) -> (String, Value) {
    let name = args["name"].as_str().unwrap_or_default().trim().to_string();
    let inner = match &args["arguments"] {
        Value::String(s) if s.trim().is_empty() => json!({}),
        Value::String(s) => serde_json::from_str::<Value>(s.trim()).unwrap_or_else(|_| Value::String(s.clone())),
        Value::Null => json!({}),
        v => v.clone(),
    };
    (name, inner)
}

/// The tool a name means: its routed name exactly; else the one tool of that name, in any case, as
/// routed or as its server names it; else the one tool the agent's own name for it ends with,
/// word for word (`mcp__janus-tools__github__search` is `github__search`).
pub(super) fn named<'a>(tools: &'a [Routed], name: &str) -> Option<&'a Routed> {
    if let Some(t) = tools.iter().find(|t| t.name == name) {
        return Some(t);
    }
    let one = |found: Vec<&'a Routed>| if found.len() == 1 { found.into_iter().next() } else { None };
    let same: Vec<&Routed> =
        tools.iter().filter(|t| t.name.eq_ignore_ascii_case(name) || t.tool.eq_ignore_ascii_case(name)).collect();
    if !same.is_empty() {
        return one(same);
    }
    let words_of = |s: &str| -> Vec<String> {
        s.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_string).collect()
    };
    let asked = words_of(name);
    one(tools
        .iter()
        .filter(|t| {
            let want = words_of(&t.name).concat();
            (1..asked.len()).any(|k| asked[k..].concat() == want)
        })
        .collect())
}

/// Why a name means no tool: none is called that, and the tools close to it.
pub(super) fn no_such(tools: &[Routed], name: &str, lang: Lang) -> String {
    let close = near(tools, name);
    let mut why = tr!(lang, "没有叫 {name} 的工具。", "No tool is called {name}.");
    if close.is_empty() {
        why += &tr!(lang, "find_tools 给出工具的名字。", " find_tools gives the tools' names.");
    } else {
        let close: Vec<&Routed> = close.into_iter().take(FULL).collect();
        why += &format!("{}\n{}", tr!(lang, "相近的：", " Close to it:"), name_lines(&close));
    }
    why
}

/// `find_tools` and `call_tool` as `tools/list` gives them; `about` is what `find_tools` says of
/// the servers behind it (`say::find_description`).
pub(super) fn specs(about: String, lang: Lang) -> [Value; 2] {
    [
        json!({
            "name": FIND,
            "description": about,
            "inputSchema": { "type": "object", "properties": {
                "query": { "type": "string", "description": lang.pick("要做的事，或工具名；为空时列出所有工具", "Words of what you want done, or a tool's name; empty names every tool") },
                "server": { "type": "string", "description": lang.pick("只找这个服务器的工具", "Only this server's tools") },
            } },
        }),
        json!({
            "name": CALL,
            "description": lang.pick(
                "调用这个服务器背后的 MCP 服务器的一个工具：名字是 find_tools 给出的（服务器__工具），参数按它的 inputSchema。",
                "Runs a tool of the MCP servers behind this one, by the name find_tools gives it (<server>__<tool>), with arguments as its inputSchema says.",
            ),
            "inputSchema": { "type": "object", "properties": {
                "name": { "type": "string", "description": lang.pick("工具名，服务器__工具", "The tool's name, <server>__<tool>") },
                "arguments": { "type": "object", "description": lang.pick("工具的参数，按它的 inputSchema", "The tool's arguments, as its inputSchema says") },
            }, "required": ["name"] },
        }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::{Body, Item, McpServer, Origin};

    fn tool(server: &str, name: &str, desc: &str) -> Routed {
        let item = Item {
            id: server.into(),
            name: server.into(),
            description: String::new(),
            origin: Origin::Created,
            revision: "r".into(),
            updated_at: 0,
            body: Body::Mcp { server: McpServer::Http { url: "http://x".into(), header_keys: vec![] } },
        };
        Routed {
            name: super::super::routed_name(server, name),
            tool: name.into(),
            item,
            spec: json!({ "name": name, "description": desc, "inputSchema": { "type": "object" } }),
        }
    }

    fn tools() -> Vec<Routed> {
        vec![
            tool("github", "create_issue", "Create a new issue in a GitHub repository"),
            tool("github", "search_repositories", "Search for GitHub repositories"),
            tool("chrome", "take_screenshot", "Take a screenshot of the page or an element"),
            tool("fs", "read_file", "Read the contents of a file"),
            tool("feishu", "send_message", "发送飞书消息给用户或群"),
        ]
    }

    fn names(ms: Vec<&Routed>) -> Vec<String> {
        ms.into_iter().map(|t| t.name.clone()).collect()
    }

    #[test]
    fn a_query_finds_the_tool_that_does_it_and_not_a_near_one() {
        let ts = tools();
        assert_eq!(names(matches(&ts, "create an issue"))[0], "github__create_issue");
        assert_eq!(names(matches(&ts, "search repos")), ["github__search_repositories"]);
        assert!(matches(&ts, "send an email").is_empty(), "a word or two of a longer ask is no match");
        assert_eq!(names(matches(&ts, "github__create_issue")), ["github__create_issue"], "the name itself");
    }

    #[test]
    fn a_chinese_query_finds_chinese_and_english_tools() {
        let ts = tools();
        assert_eq!(names(matches(&ts, "发送飞书消息")), ["feishu__send_message"]);
        assert_eq!(
            names(matches(&ts, "帮我截个图 screenshot")),
            ["chrome__take_screenshot"],
            "截图 does not count against English"
        );
        assert_eq!(key_words("帮我截个图"), ["截", "图"]);
    }

    #[test]
    fn nothing_found_names_what_there_is() {
        let ts = tools();
        let (said, failed) = find(&ts, &json!({ "query": "book a flight" }), Lang::En);
        assert!(!failed);
        assert!(
            said.contains("No tool was found") && said.contains("github (2: create_issue, search_repositories)"),
            "{said}"
        );
        let (all, _) = find(&ts, &json!({}), Lang::En);
        assert!(all.contains("- fs__read_file: Read the contents of a file"), "{all}");
        let (one, _) = find(&ts, &json!({ "query": "screenshot", "server": "nope" }), Lang::En);
        assert!(one.contains("No server called nope"), "{one}");
    }

    #[test]
    fn found_tools_come_with_their_parameters() {
        let (said, _) = find(&tools(), &json!({ "query": "read a file" }), Lang::En);
        assert!(said.contains("\"name\": \"fs__read_file\"") && said.contains("inputSchema"), "{said}");
    }

    #[test]
    fn call_tool_takes_the_name_an_agent_gives() {
        let ts = tools();
        assert_eq!(named(&ts, "github__create_issue").unwrap().name, "github__create_issue");
        assert_eq!(named(&ts, "create_issue").unwrap().name, "github__create_issue", "the server's own name");
        assert_eq!(named(&ts, "mcp__janus-tools__github__create_issue").unwrap().name, "github__create_issue");
        assert!(named(&ts, "s__create_issue").is_none(), "words, not characters");
        assert!(no_such(&ts, "create_isue", Lang::En).contains("github__create_issue"));
        let (name, inner) = call_args(&json!({ "name": " fs__read_file ", "arguments": "{\"path\": \"a\"}" }));
        assert_eq!((name.as_str(), inner), ("fs__read_file", json!({ "path": "a" })));
    }
}
