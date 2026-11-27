//! MCP servers in the library: their definitions, the values of their environment variables and
//! HTTP headers (kept apart, in `secrets/library/`), and the values a server is started or called
//! with.

use super::reserved_mcp_name;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// How an agent reaches a server. Only variable and header names live here; their values are in
/// the server's own secrets file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "transport", rename_all = "lowercase", rename_all_fields = "camelCase")]
pub enum McpServer {
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env_keys: Vec<String>,
    },
    /// Streamable HTTP.
    Http {
        url: String,
        #[serde(default)]
        header_keys: Vec<String>,
    },
    Sse {
        url: String,
        #[serde(default)]
        header_keys: Vec<String>,
    },
}

impl McpServer {
    /// The names whose values the server needs: environment variables or HTTP headers.
    pub fn keys(&self) -> &[String] {
        match self {
            McpServer::Stdio { env_keys, .. } => env_keys,
            McpServer::Http { header_keys, .. } | McpServer::Sse { header_keys, .. } => header_keys,
        }
    }

    /// A server reached at `url` by `transport`: `http` (also written `streamable-http`) or `sse`.
    pub fn remote(transport: &str, url: String, header_keys: Vec<String>) -> Result<McpServer> {
        check_url(&url)?;
        Ok(match transport {
            "http" | "streamable-http" => McpServer::Http { url, header_keys },
            "sse" => McpServer::Sse { url, header_keys },
            other => bail!("不支持的传输方式：{other}"),
        })
    }

    /// The values the server is started or called with: those of its declared keys that are set,
    /// environment variables for a command and headers for a URL.
    pub fn set_values(&self, values: &McpValues) -> Vec<(String, String)> {
        let from = values.for_server(self);
        self.keys()
            .iter()
            .filter_map(|k| from.get(k).filter(|v| !v.is_empty()).map(|v| (k.clone(), v.clone())))
            .collect()
    }
}

/// A server's secret values: environment variables for a command, headers for a URL.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpValues {
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
}

impl McpValues {
    /// The map that holds this server's values.
    pub fn for_server(&self, server: &McpServer) -> &BTreeMap<String, String> {
        match server {
            McpServer::Stdio { .. } => &self.env,
            _ => &self.headers,
        }
    }

    /// The map that holds this server's values, to change them.
    pub fn of(&mut self, server: &McpServer) -> &mut BTreeMap<String, String> {
        match server {
            McpServer::Stdio { .. } => &mut self.env,
            _ => &mut self.headers,
        }
    }

    /// Only the server's own keys, so nothing it no longer declares lingers.
    pub fn kept_for(mut self, server: &McpServer) -> McpValues {
        let keys = server.keys().to_vec();
        let mut out = McpValues::default();
        for (k, v) in std::mem::take(self.of(server)) {
            if keys.contains(&k) {
                out.of(server).insert(k, v);
            }
        }
        out
    }
}

/// One server read from a pasted config or a manifest: what it is and the values it named.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub name: String,
    pub description: String,
    pub server: McpServer,
    pub values: McpValues,
}

/// MCP server names: letters, digits, `-` and `_`; `janus` belongs to the board.
pub(crate) fn check_name(name: &str) -> Result<()> {
    if name.is_empty() || name.len() > 64 || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        bail!("MCP 名称只能包含字母、数字、- 和 _（最多 64 个字符）：{name}");
    }
    if reserved_mcp_name(name) {
        bail!("MCP 名称 {name} 保留给 Janus（看板和 MCP 路由），请换一个名字");
    }
    Ok(())
}

/// Read a config someone pasted or a repository holds: Claude's `mcpServers`, VS Code's `servers`,
/// or one server object with a `name`. Entries that can't be used come back as warnings.
pub fn parse_config(text: &str) -> Result<(Vec<Found>, Vec<String>)> {
    let doc: Value = serde_json::from_str(text).context("不是有效的 JSON")?;
    let obj = doc.as_object().context("MCP 配置必须是一个 JSON 对象")?;
    let entries: Vec<(String, Value)> = if let Some(m) = obj.get("mcpServers").and_then(Value::as_object) {
        m.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    } else if let Some(m) = obj.get("servers").and_then(Value::as_object) {
        m.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    } else if doc["name"].as_str().is_some_and(|n| !n.trim().is_empty())
        && (doc["command"].is_string() || doc["url"].is_string())
    {
        vec![(doc["name"].as_str().unwrap_or_default().to_string(), doc.clone())]
    } else {
        bail!("没有 mcpServers、servers 或单个服务器定义");
    };
    let mut found = vec![];
    let mut warnings = vec![];
    for (name, def) in entries {
        match parse_server(name.trim(), &def) {
            Ok(f) => found.push(f),
            Err(e) => warnings.push(format!("{name}：{e:#}")),
        }
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    Ok((found, warnings))
}

fn parse_server(name: &str, def: &Value) -> Result<Found> {
    check_name(name)?;
    let ty = def["type"].as_str().unwrap_or("").to_ascii_lowercase();
    let command = def["command"].as_str().unwrap_or("").trim().to_string();
    let url = def["url"].as_str().unwrap_or("").trim().to_string();
    let (server, values) = if !command.is_empty() && !matches!(ty.as_str(), "http" | "streamable-http" | "sse") {
        let env = pairs(def, "env");
        let args = def["args"]
            .as_array()
            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect())
            .unwrap_or_default();
        let server = McpServer::Stdio { command, args, env_keys: env.keys().cloned().collect() };
        (server, McpValues { env, ..Default::default() })
    } else {
        let headers = pairs(def, "headers");
        let header_keys = headers.keys().cloned().collect();
        let server = McpServer::remote(if ty == "sse" { "sse" } else { "http" }, url, header_keys)?;
        (server, McpValues { headers, ..Default::default() })
    };
    let description = def["description"].as_str().unwrap_or("").trim().to_string();
    Ok(Found { name: name.to_string(), description, server, values })
}

pub(super) fn check_url(url: &str) -> Result<()> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        bail!("服务器地址必须以 http:// 或 https:// 开头");
    }
    Ok(())
}

/// A JSON object of scalar values as strings, by key.
fn pairs(def: &Value, field: &str) -> BTreeMap<String, String> {
    def[field]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(k, v)| {
            let k = k.trim();
            let v = match v {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                Value::Bool(b) => b.to_string(),
                _ => return None,
            };
            (!k.is_empty()).then(|| (k.to_string(), v))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pasted_configs_in_every_usual_shape() {
        let (found, warnings) = parse_config(
            r#"{"mcpServers": {
                "github": {"command": "npx", "args": ["-y", "gh-mcp"], "env": {"GITHUB_TOKEN": "t0k"}},
                "docs": {"type": "http", "url": "https://docs.example/mcp", "headers": {"Authorization": "Bearer x"}},
                "events": {"type": "sse", "url": "https://events.example/sse"},
                "janus": {"command": "x"},
                "bad": {"url": "ftp://nope"}
            }}"#,
        )
        .unwrap();
        let names: Vec<&str> = found.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["docs", "events", "github"]);
        assert_eq!(warnings.len(), 2, "{warnings:?}");
        let github = &found[2];
        assert_eq!(github.server.keys(), ["GITHUB_TOKEN"]);
        assert_eq!(github.values.env["GITHUB_TOKEN"], "t0k");
        assert!(matches!(found[1].server, McpServer::Sse { .. }));
        let (vs, _) = parse_config(r#"{"servers": {"a": {"command": "node"}}}"#).unwrap();
        assert_eq!(vs[0].name, "a");
        let (one, _) = parse_config(r#"{"name": "solo", "url": "https://x.example/mcp"}"#).unwrap();
        assert!(matches!(one[0].server, McpServer::Http { .. }));
        assert!(parse_config("[]").is_err());
    }

    /// The definition serializes with names only; the server gets the declared values that are set.
    #[test]
    fn servers_get_only_declared_values() {
        let server = McpServer::Stdio {
            command: "node".into(),
            args: vec!["s.js".into()],
            env_keys: vec!["TOKEN".into(), "EMPTY".into()],
        };
        let values = McpValues {
            env: BTreeMap::from([
                ("TOKEN".into(), "v".into()),
                ("EMPTY".into(), String::new()),
                ("OTHER".into(), "o".into()),
            ]),
            ..Default::default()
        };
        assert_eq!(server.set_values(&values), [("TOKEN".to_string(), "v".to_string())]);
        assert_eq!(values.clone().kept_for(&server).env.len(), 2);
        let text = serde_json::to_string(&server).unwrap();
        assert_eq!(text, r#"{"transport":"stdio","command":"node","args":["s.js"],"envKeys":["TOKEN","EMPTY"]}"#);
        let http = McpServer::Http { url: "https://x/mcp".into(), header_keys: vec!["Authorization".into()] };
        let v =
            McpValues { headers: BTreeMap::from([("Authorization".into(), "Bearer t".into())]), ..Default::default() };
        assert_eq!(http.set_values(&v), [("Authorization".to_string(), "Bearer t".to_string())]);
    }
}
