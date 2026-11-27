//! An MCP server being written in the library's editor: the same server as a form or as JSON in
//! Claude Code's `.mcp.json` shape, switched freely while the JSON reads as one server. The editor
//! sends its draft with what was just typed (`POST /api/library/mcp/draft`) and draws what comes
//! back: the command line taken apart, the JSON read into the form, the form written as JSON.
// @project-doc docs/domains/20261116-resource_library.md#mcp

use crate::lang::Lang;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

/// An environment variable (stdio) or HTTP header (http, sse) and its value.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Var {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub value: String,
}

/// The server in the editor.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Draft {
    /// The library item being edited; empty for a new server.
    pub id: String,
    pub name: String,
    pub description: String,
    /// `stdio`, `http` or `sse`.
    pub transport: String,
    pub command: String,
    pub args: Vec<String>,
    pub url: String,
    pub vars: Vec<Var>,
}

impl Default for Draft {
    fn default() -> Draft {
        Draft {
            id: String::new(),
            name: String::new(),
            description: String::new(),
            transport: "stdio".into(),
            command: String::new(),
            args: vec![],
            url: String::new(),
            vars: vec![],
        }
    }
}

impl Draft {
    /// A saved server (`GET /api/library/items/{id}`) with the values it holds
    /// (`GET /api/library/mcp/{id}/values`).
    pub fn of_item(item: &Value, values: &Value) -> Draft {
        let srv = &item["server"];
        let s = |v: &Value| v.as_str().unwrap_or("").to_string();
        Draft {
            id: s(&item["id"]),
            name: s(&item["name"]),
            description: s(&item["description"]),
            transport: match srv["transport"].as_str() {
                Some(t @ ("http" | "sse")) => t.into(),
                _ => "stdio".into(),
            },
            command: s(&srv["command"]),
            args: srv["args"].as_array().into_iter().flatten().map(s).collect(),
            url: s(&srv["url"]),
            vars: item["vars"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|v| Var { name: s(&v["name"]), value: s(&values[s(&v["name"])]) })
                .collect(),
        }
    }

    /// The command and its arguments as one would type them, quoted where needed.
    pub fn line(&self) -> String {
        if self.command.is_empty() && self.args.is_empty() {
            return String::new();
        }
        std::iter::once(&self.command).chain(&self.args).map(|w| quote(w)).collect::<Vec<_>>().join(" ")
    }

    /// Take a typed command line apart; an unclosed quote keeps the words read so far.
    pub fn set_line(&mut self, text: &str) {
        let closed = if text.contains('"') { format!("{text}\"") } else { format!("{text}'") };
        let words =
            crate::agents::split_command(text).or_else(|_| crate::agents::split_command(&closed)).unwrap_or_default();
        let mut words = words.into_iter();
        self.command = words.next().unwrap_or_default();
        self.args = words.collect();
    }

    /// The server as Claude Code's `.mcp.json` has it, values included.
    pub fn json(&self) -> String {
        let pairs: Map<String, Value> =
            self.vars.iter().filter(|v| !v.name.is_empty()).map(|v| (v.name.clone(), json!(v.value))).collect();
        let entry = if self.transport == "stdio" {
            json!({ "type": "stdio", "command": self.command, "args": self.args, "env": pairs })
        } else {
            json!({ "type": self.transport, "url": self.url, "headers": pairs })
        };
        let mut servers = Map::new();
        servers.insert(self.name.clone(), entry);
        serde_json::to_string_pretty(&json!({ "mcpServers": servers })).unwrap_or_default()
    }

    /// Take the server read from the JSON view. The item, its description (when the JSON has
    /// none), how it runs and the order of its variables stay.
    pub fn take(&mut self, read: Draft) {
        if !read.name.is_empty() {
            self.name = read.name;
        }
        if !read.description.is_empty() {
            self.description = read.description;
        }
        self.transport = read.transport;
        self.command = read.command;
        self.args = read.args;
        self.url = read.url;
        // JSON objects have no order: the variables keep the form's, new ones after.
        let before = |name: &str| self.vars.iter().position(|v| v.name == name);
        let mut vars: Vec<(usize, Var)> = read
            .vars
            .into_iter()
            .enumerate()
            .map(|(i, v)| (before(&v.name).unwrap_or(self.vars.len() + i), v))
            .collect();
        vars.sort_by_key(|(at, _)| *at);
        self.vars = vars.into_iter().map(|(_, v)| v).collect();
    }

    /// The body of `POST /api/library/mcp` and `PUT /api/library/mcp/{id}`: the arguments as a
    /// list, so the service doesn't split the command again, and every value as it stands.
    pub fn body(&self) -> Value {
        let vars: Vec<Value> = self
            .vars
            .iter()
            .filter(|v| !v.name.trim().is_empty())
            .map(|v| json!({ "name": v.name.trim(), "value": v.value }))
            .collect();
        json!({
            "name": self.name.trim(), "description": self.description, "transport": self.transport,
            "command": self.command, "args": self.args, "url": self.url.trim(), "vars": vars,
        })
    }
}

/// A word of a command line, quoted when it has a space or a quote.
fn quote(word: &str) -> String {
    if !word.is_empty() && !word.chars().any(|c| c.is_whitespace() || c == '"' || c == '\'') {
        word.to_string()
    } else if word.contains('\'') {
        format!("\"{word}\"")
    } else {
        format!("'{word}'")
    }
}

/// The text without the commas READMEs leave before a `}` or `]`.
fn forgive_trailing_commas(text: &str) -> String {
    let chars: Vec<char> = text.trim().chars().collect();
    let mut out = String::with_capacity(chars.len());
    for (i, c) in chars.iter().enumerate() {
        if *c == ',' && chars[i + 1..].iter().find(|c| !c.is_whitespace()).is_some_and(|n| *n == '}' || *n == ']') {
            continue;
        }
        out.push(*c);
    }
    out
}

/// The servers in pasted or typed JSON: a README's `{"mcpServers": {…}}`, VS Code's
/// `{"servers": {…}}`, one server with a `name`, or one server's entry alone. None when the text
/// isn't JSON holding a server.
pub fn servers_in(text: &str) -> Option<Vec<Draft>> {
    let doc: Value = serde_json::from_str(&forgive_trailing_commas(text)).ok()?;
    let obj = doc.as_object()?;
    let found: Vec<Draft> =
        if let Some(map) = obj.get("mcpServers").or_else(|| obj.get("servers")).and_then(Value::as_object) {
            // Listed by name: a JSON object has no order of its own.
            let mut names: Vec<&String> = map.keys().collect();
            names.sort();
            names.into_iter().filter_map(|name| entry(name, &map[name])).collect()
        } else if obj.contains_key("command") || obj.contains_key("url") {
            entry(doc["name"].as_str().unwrap_or(""), &doc).into_iter().collect()
        } else {
            return None;
        };
    (!found.is_empty()).then_some(found)
}

fn entry(name: &str, e: &Value) -> Option<Draft> {
    let mut d = Draft {
        name: name.trim().to_string(),
        description: e["description"].as_str().unwrap_or("").into(),
        ..Draft::default()
    };
    let values = |field: &str| -> Vec<Var> {
        e[field]
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(k, v)| {
                let value = match v {
                    Value::String(s) => s.clone(),
                    Value::Number(n) => n.to_string(),
                    Value::Bool(b) => b.to_string(),
                    _ => return None,
                };
                Some(Var { name: k.clone(), value })
            })
            .collect()
    };
    let ty = e["type"].as_str().unwrap_or("").to_lowercase();
    let url = e["url"].as_str().unwrap_or("");
    if !url.is_empty() && !(ty == "stdio" && !e["command"].is_null()) {
        d.transport = if ty == "sse" { "sse".into() } else { "http".into() };
        d.url = url.into();
        d.vars = values("headers");
    } else if e["command"].as_str().is_some_and(|c| !c.is_empty())
        || e["command"].as_array().is_some_and(|a| !a.is_empty())
    {
        let mut parts: Vec<String> = match &e["command"] {
            Value::Array(a) => a.iter().map(|x| x.as_str().unwrap_or("").to_string()).collect(),
            c => vec![c.as_str().unwrap_or("").to_string()],
        };
        parts.extend(e["args"].as_array().into_iter().flatten().map(|a| a.as_str().unwrap_or("").to_string()));
        d.transport = "stdio".into();
        d.command = parts.remove(0);
        d.args = parts;
        d.vars = values("env");
    } else {
        return None;
    }
    Some(d)
}

/// What the editor draws after an edit (`POST /api/library/mcp/draft`): the draft with the typed
/// command line (`line`) or JSON (`text`) taken in, written both ways, the body to save it with,
/// the servers a new server's JSON holds when it holds more than one, and why the JSON can't be
/// taken (`error`), while it can't.
pub fn edit(mut d: Draft, line: Option<&str>, text: Option<&str>, lang: Lang) -> Value {
    let mut error = Value::Null;
    let mut several: Vec<String> = vec![];
    if let Some(line) = line {
        d.set_line(line);
    }
    if let Some(text) = text {
        match servers_in(text) {
            None => error = json!(lang.pick("这不是 MCP 服务器的 JSON", "This is not an MCP server's JSON")),
            Some(found) if found.len() > 1 && !d.id.is_empty() => {
                error = json!(lang.pick("这里只能是这一个服务器", "Only this one server can be here"));
            }
            Some(found) if found.len() > 1 => several = found.into_iter().map(|f| f.name).collect(),
            Some(mut found) => d.take(found.remove(0)),
        }
    }
    json!({ "line": d.line(), "json": d.json(), "body": d.body(), "several": several, "error": error, "draft": d })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item() -> Value {
        json!({
            "id": "i1", "name": "github", "description": "代码托管",
            "server": { "transport": "stdio", "command": "npx", "args": ["-y", "gh mcp"] },
            "vars": [{ "name": "GITHUB_TOKEN", "set": true }, { "name": "EMPTY", "set": false }],
        })
    }

    /// The editor's form and JSON are one server, values included: written out as Claude Code's
    /// `.mcp.json` and read back the same.
    #[test]
    fn a_server_reads_the_same_as_form_or_json() {
        let d = Draft::of_item(&item(), &json!({ "GITHUB_TOKEN": "ghp-1" }));
        assert_eq!(d.line(), "npx -y 'gh mcp'");
        assert_eq!(
            d.vars,
            [
                Var { name: "GITHUB_TOKEN".into(), value: "ghp-1".into() },
                Var { name: "EMPTY".into(), value: "".into() }
            ]
        );
        let back = servers_in(&d.json()).unwrap();
        assert_eq!(back.len(), 1);
        let mut again = Draft { id: d.id.clone(), description: d.description.clone(), ..back[0].clone() };
        again.vars = d.vars.clone();
        assert_eq!(again, d, "the JSON reads back as the form");
        assert!(d.json().contains("\"GITHUB_TOKEN\": \"ghp-1\""), "the value is in the JSON: {}", d.json());
    }

    #[test]
    fn a_command_line_is_taken_apart_as_typed() {
        let mut d = Draft::default();
        d.set_line("npx -y \"@scope/server name\" --flag");
        assert_eq!(
            (d.command.as_str(), d.args.clone()),
            ("npx", vec!["-y".to_string(), "@scope/server name".into(), "--flag".into()])
        );
        d.set_line("uvx 'half quoted");
        assert_eq!(d.args, ["half quoted"], "an unclosed quote keeps what was typed");
        assert_eq!(quote("it's"), "\"it's\"");
        assert_eq!(quote(""), "''");
    }

    #[test]
    fn pasted_json_holds_one_server_or_several() {
        let readme = r#"{ "mcpServers": { "b": { "command": ["uvx", "b-mcp"], "args": ["--x"], "env": { "N": 1, "OK": true } }, "a": { "type": "sse", "url": "https://a/sse", "headers": { "Authorization": "Bearer t" } }, } }"#;
        let found = servers_in(readme).unwrap();
        assert_eq!(found.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!((found[0].transport.as_str(), found[0].url.as_str()), ("sse", "https://a/sse"));
        assert_eq!(
            (found[1].command.as_str(), found[1].args.clone()),
            ("uvx", vec!["b-mcp".to_string(), "--x".into()])
        );
        assert_eq!(
            found[1].vars,
            [Var { name: "N".into(), value: "1".into() }, Var { name: "OK".into(), value: "true".into() }]
        );
        assert_eq!(servers_in(r#"{"servers": {"v": {"url": "https://v"}}}"#).unwrap()[0].transport, "http");
        assert_eq!(servers_in(r#"{"command": "x"}"#).unwrap()[0].name, "", "one entry alone");
        assert!(
            servers_in("not json").is_none()
                && servers_in("{}").is_none()
                && servers_in(r#"{"mcpServers": {}}"#).is_none()
        );
    }

    #[test]
    fn taking_json_keeps_the_forms_order_and_description() {
        let mut d = Draft::of_item(&item(), &json!({}));
        let read = servers_in(
            r#"{"mcpServers": {"github": {"command": "npx", "env": {"NEW": "n", "EMPTY": "e", "GITHUB_TOKEN": "t"}}}}"#,
        )
        .unwrap();
        d.take(read[0].clone());
        assert_eq!(d.vars.iter().map(|v| v.name.as_str()).collect::<Vec<_>>(), ["GITHUB_TOKEN", "EMPTY", "NEW"]);
        assert_eq!(d.description, "代码托管", "the JSON had none");
        assert_eq!(d.id, "i1");
    }

    #[test]
    fn an_edit_comes_back_written_both_ways() {
        let out = edit(Draft::default(), Some("npx x"), None, Lang::Zh);
        assert_eq!(out["draft"]["command"], "npx");
        assert_eq!(out["line"], "npx x");
        assert!(out["error"].is_null());
        let bad = edit(Draft::default(), None, Some("{"), Lang::En);
        assert_eq!(bad["error"], "This is not an MCP server's JSON");
        let two = r#"{"mcpServers": {"a": {"command": "a"}, "b": {"command": "b"}}}"#;
        assert_eq!(edit(Draft::default(), None, Some(two), Lang::Zh)["several"], json!(["a", "b"]));
        let editing = Draft { id: "i1".into(), ..Draft::default() };
        assert_eq!(edit(editing, None, Some(two), Lang::Zh)["error"], "这里只能是这一个服务器");
        let named =
            Draft { name: " fs ".into(), vars: vec![Var { name: " ".into(), value: "x".into() }], ..Draft::default() };
        assert_eq!(edit(named, None, None, Lang::Zh)["body"]["name"], "fs");
    }
}
