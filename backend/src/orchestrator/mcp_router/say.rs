//! What `janus-tools` says of the servers behind it, after magpie's MCP router (`HubLine`,
//! `hubDetails`): a line naming each server and what it is for, then a line for each with its
//! tools, within a budget. The agent sees only `find_tools`, `call_tool` and `run_code`, so these
//! words, in `initialize`'s instructions and in `find_tools`' description, are how its model
//! learns what there is to find.
// @project-doc docs/domains/20261116-resource_library.md#mcp_router

use crate::lang::{Lang, tr};

/// How much of the first line, naming every server, an agent is sure to show: Pi shows a server
/// as its instructions' first line cut at 250 characters, and magpie keeps to it.
const LINE_MAX: usize = 250;
/// How long the instructions are at most: half of the 4096 characters Claude Code keeps of a
/// server's instructions.
pub(super) const SAY_MAX: usize = 2000;
/// How many characters of `find_tools`' description name the servers' tools: about three tools'
/// schemas, in place of them all.
pub(super) const FIND_SAY_MAX: usize = 1200;

/// One server behind the router, as its words name it.
pub(super) struct Server {
    pub name: String,
    /// Its own title (`serverInfo.title`).
    pub title: String,
    /// What it is for: the library's description, else what the server says of itself.
    pub about: String,
    /// Its tools by their own names, each with its description.
    pub tools: Vec<(String, String)>,
}

fn len(s: &str) -> usize {
    s.chars().count()
}

/// `s` on one line.
fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `s` in at most `n` characters, cut at a word (or anywhere in a script without spaces) with `…`.
pub(super) fn cut(s: &str, n: usize) -> String {
    if len(s) <= n {
        return s.to_string();
    }
    let c: String = s.chars().take(n.saturating_sub(1)).collect();
    let c = match c.rfind(' ') {
        Some(i) if i > 0 => c[..i].to_string(),
        _ => c,
    };
    format!("{}…", c.trim_end_matches([' ', ',', '，', '、']))
}

/// The first part of a description: up to its first comma, colon, semicolon or full stop outside
/// brackets, so a line keeps what a thing is and not how it is used.
pub(super) fn clause(s: &str) -> String {
    let s = one_line(s);
    let mut depth = 0usize;
    for (i, c) in s.char_indices() {
        match c {
            '(' | '[' | '（' => depth += 1,
            ')' | ']' | '）' if depth > 0 => depth -= 1,
            ',' | ':' | ';' | '，' | '：' | '；' | '。' if depth == 0 && i > 0 => {
                return s[..i].trim_end_matches('.').to_string();
            }
            '.' if depth == 0 && i > 0 && s[i..].starts_with(". ") => return s[..i].to_string(),
            _ => {}
        }
    }
    s.trim_end_matches(['.', '。']).to_string()
}

/// What a tool does, in a few words: the first clause of its description, a router's own
/// `[<server>] ` before it left out.
pub(super) fn tool_clause(d: &str) -> String {
    let d = one_line(d);
    let d = match d.strip_prefix('[').and_then(|r| r.split_once("] ")) {
        Some((_, rest)) => rest.to_string(),
        None => d,
    };
    cut(&clause(&d), 80)
}

/// One line naming every server, each with what it is for cut to fit `LINE_MAX`: as many names as
/// fit first, the rest counted; what is left goes to the shortest abouts whole, the others an equal
/// share cut at a word. Empty with no server.
pub(super) fn line(servers: &[Server], lang: Lang) -> String {
    if servers.is_empty() {
        return String::new();
    }
    let head = tr!(lang, "Janus 资源库的 MCP 服务器：", "Tools of the Janus library's MCP servers: ");
    let sep = lang.pick("、", ", ");
    let (open, close) = (lang.pick("（", " ("), lang.pick("）", ")"));
    let end = lang.pick("。", ".");
    let room = LINE_MAX - len(end);
    let build = |abouts: &[String], kept: usize| {
        let mut parts: Vec<String> = servers[..kept]
            .iter()
            .zip(abouts)
            .map(|(s, a)| if a.is_empty() { s.name.clone() } else { format!("{}{open}{a}{close}", s.name) })
            .collect();
        let more = servers.len() - kept;
        if more > 0 {
            parts.push(tr!(lang, "另有 {more} 个", "and {more} more"));
        }
        format!("{head}{}", parts.join(sep))
    };
    let none = vec![String::new(); servers.len()];
    let mut kept = servers.len();
    while kept > 0 && len(&build(&none, kept)) > room {
        kept -= 1;
    }
    let short: Vec<String> = servers.iter().map(|s| clause(&s.about)).collect();
    let mut abouts = vec![String::new(); servers.len()];
    let mut left = room.saturating_sub(len(&build(&none, kept)));
    let mut order: Vec<usize> = (0..kept).filter(|&i| !short[i].is_empty()).collect();
    order.sort_by_key(|&i| len(&short[i]));
    let wrap = len(open) + len(close);
    for (j, &i) in order.iter().enumerate() {
        let share = (left / (order.len() - j)).saturating_sub(wrap);
        if share < 12 {
            continue;
        }
        abouts[i] = cut(&short[i], share);
        left = left.saturating_sub(len(&abouts[i]) + wrap);
    }
    format!("{}{end}", build(&abouts, kept))
}

/// A line for each server, within `max` characters in all: its name, its title, what it is for and
/// its tools with the first clause of each description. A line too long for its share names its
/// tools only, then as many as fit and how many more; the room a short line leaves goes to the
/// others.
pub(super) fn details(servers: &[Server], max: usize, lang: Lang) -> String {
    let full: Vec<String> = servers.iter().map(|s| server_line(s, usize::MAX, lang)).collect();
    let mut order: Vec<usize> = (0..servers.len()).filter(|&i| !full[i].is_empty()).collect();
    order.sort_by_key(|&i| len(&full[i]));
    let mut lines = vec![String::new(); servers.len()];
    let mut left = max;
    for (j, &i) in order.iter().enumerate() {
        let share = (left / (order.len() - j)).saturating_sub(1);
        let l = server_line(&servers[i], share, lang);
        if !l.is_empty() {
            left = left.saturating_sub(len(&l) + 1);
            lines[i] = l;
        }
    }
    lines.into_iter().filter(|l| !l.is_empty()).map(|l| l + "\n").collect()
}

/// One server's line in at most `room` characters: `- name (title): what it is for. Tools: a
/// (what a does), b.`; empty when nothing is known of it or not even its name fits.
fn server_line(s: &Server, room: usize, lang: Lang) -> String {
    let (open, close) = (lang.pick("（", " ("), lang.pick("）", ")"));
    let mut head = format!("- {}", s.name);
    let title = one_line(&s.title);
    if !title.is_empty() && !title.eq_ignore_ascii_case(&s.name) {
        head += &format!("{open}{title}{close}");
    }
    let about = one_line(&s.about).trim_end_matches(['.', '。']).to_string();
    if about.is_empty() && s.tools.is_empty() {
        return String::new();
    }
    let (colon, stop, sep) = (lang.pick("：", ": "), lang.pick("。", ". "), lang.pick("、", ", "));
    if len(&head) + len(colon) + 1 > room {
        return String::new();
    }
    let say = |about: &str, tools: &[String], more: usize| {
        let mut parts = vec![];
        if !about.is_empty() {
            parts.push(about.to_string());
        }
        match (tools.is_empty(), more) {
            (true, 0) => {}
            (true, m) => parts.push(tr!(lang, "{m} 个工具", "{m} tools")),
            (false, 0) => parts.push(format!("{}{}", lang.pick("工具：", "Tools: "), tools.join(sep))),
            (false, m) => parts.push(format!(
                "{}{}{}",
                lang.pick("工具：", "Tools: "),
                tools.join(sep),
                tr!(lang, " 等，另有 {m} 个", ", and {m} more")
            )),
        }
        format!("{head}{colon}{}{}", parts.join(stop), lang.pick("。", "."))
    };
    let names: Vec<String> = s.tools.iter().map(|(n, _)| n.clone()).collect();
    let described: Vec<String> = s
        .tools
        .iter()
        .map(|(n, d)| {
            let c = tool_clause(d);
            if c.is_empty() { n.clone() } else { format!("{n}{open}{c}{close}") }
        })
        .collect();
    for tools in [&described, &names] {
        let l = say(&about, tools, 0);
        if len(&l) <= room {
            return l;
        }
    }
    // Too long: what it is for in half the room at most, then as many names as fit.
    let budget = (room / 2).saturating_sub(len(&head) + len(colon) + 1);
    let about = if about.is_empty() || budget < 12 { String::new() } else { cut(&about, budget) };
    for kept in (0..=names.len()).rev() {
        let l = say(&about, &names[..kept], names.len() - kept);
        if len(&l) <= room {
            return l;
        }
    }
    if about.is_empty() { String::new() } else { cut(&say(&about, &[], 0), room) }
}

/// How the agent reaches the tools, said once at the end of the instructions; `code` is whether
/// `run_code` is given.
fn how(lang: Lang, code: bool) -> String {
    let mut s = tr!(
        lang,
        "这些工具不直接列出：find_tools 按要做的事或工具名查找，给出工具的说明和参数；call_tool 按名字（服务器__工具）调用它。",
        "These tools are not listed one by one: find_tools finds them by what you want done or by name, with their descriptions and parameters, and call_tool runs one by its name (server__tool)."
    );
    if code {
        s += &tr!(
            lang,
            "run_code 运行一段调用这些工具的 JavaScript，只交回它返回和打印的内容：几次调用，或只要结果的一部分时，一步完成。",
            " run_code runs JavaScript that calls these tools and gives back only what it returns and logs: several calls, or the part of an answer you need, in one step."
        );
    }
    s
}

/// `initialize`'s instructions: the line naming every server, a line for each, then how to reach
/// their tools, within `SAY_MAX`.
pub(super) fn instructions(servers: &[Server], lang: Lang, code: bool) -> String {
    let tail = how(lang, code);
    if servers.is_empty() {
        return tr!(
            lang,
            "Janus 资源库里现在没有能用的 MCP 服务器。",
            "The Janus library has no MCP server to use now."
        );
    }
    let out = line(servers, lang) + "\n";
    let room = SAY_MAX.saturating_sub(len(&out) + len(&tail));
    format!("{out}{}{tail}", details(servers, room, lang))
}

/// `find_tools`' description: the line naming every server, what `find_tools` does, then each
/// server's tools within `FIND_SAY_MAX`. Not every agent shows the instructions, and a model told
/// only how to search searched and stopped (magpie, 11-04).
pub(super) fn find_description(servers: &[Server], lang: Lang) -> String {
    let what = tr!(
        lang,
        "查找这个服务器背后的 MCP 服务器中和 query 相符的工具：要做的事，或工具名。给出每个工具的名字、说明和参数（inputSchema），再用 call_tool 调用。query 为空时列出所有工具的名字。",
        "Finds the tools of the MCP servers behind this one that match query: words of what you want done, or a tool's name. Gives each match's name, description and parameters (inputSchema); run one with call_tool. An empty query names every tool."
    );
    let first = line(servers, lang);
    if first.is_empty() {
        return what;
    }
    format!("{first}\n{what}\n{}", details(servers, FIND_SAY_MAX, lang)).trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server(name: &str, about: &str, tools: &[(&str, &str)]) -> Server {
        Server {
            name: name.into(),
            title: String::new(),
            about: about.into(),
            tools: tools.iter().map(|(n, d)| (n.to_string(), d.to_string())).collect(),
        }
    }

    #[test]
    fn instructions_name_every_server_and_tool_within_the_budget() {
        let servers = vec![
            server(
                "exa",
                "Web search and crawling, for current information.",
                &[("web_search_exa", "Search the web. Returns results.")],
            ),
            server(
                "context7",
                "Up-to-date docs for libraries",
                &[("resolve-library-id", "Find a library's id"), ("get-library-docs", "Fetch docs")],
            ),
        ];
        let say = instructions(&servers, Lang::En, true);
        assert!(say.starts_with("Tools of the Janus library's MCP servers: exa (Web search and crawling), context7 (Up-to-date docs for libraries)."), "{say}");
        assert!(
            say.contains(
                "- exa: Web search and crawling, for current information. Tools: web_search_exa (Search the web)."
            ),
            "{say}"
        );
        assert!(say.contains("find_tools") && say.contains("call_tool") && say.contains("run_code"), "{say}");
        let zh = instructions(&servers, Lang::Zh, false);
        assert!(zh.starts_with("Janus 资源库的 MCP 服务器：exa（Web search and crawling）、context7（"), "{zh}");
        assert!(!zh.contains("run_code"), "no code mode, no word of it");
    }

    #[test]
    fn a_large_library_is_cut_to_names_and_counts() {
        let tools: Vec<(String, String)> =
            (0..90).map(|i| (format!("tool_number_{i}"), format!("Does thing {i}, at length"))).collect();
        let servers: Vec<Server> = (0..30)
            .map(|i| Server {
                name: format!("server{i}"),
                title: String::new(),
                about: "A server with a long description of what it does for the agent".into(),
                tools: tools.clone(),
            })
            .collect();
        let say = instructions(&servers, Lang::En, true);
        assert!(len(&say) <= SAY_MAX, "{} characters", len(&say));
        assert!(len(line(&servers, Lang::En).as_str()) <= LINE_MAX);
        assert!(say.contains("more"), "what does not fit is counted: {say}");
        assert!(len(&find_description(&servers, Lang::En)) <= FIND_SAY_MAX + LINE_MAX + 400);
    }

    #[test]
    fn clauses_stop_at_the_first_break_outside_brackets() {
        assert_eq!(clause("Search issues (by repo, label). Returns a list."), "Search issues (by repo, label)");
        assert_eq!(clause("网页搜索，返回结果"), "网页搜索");
        assert_eq!(tool_clause("[github] Create an issue: give a title"), "Create an issue");
        assert_eq!(cut("one two three four", 10), "one two…");
    }
}
