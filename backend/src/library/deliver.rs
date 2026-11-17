//! How a member's resources reach its sessions. When a run starts, the member's skills are copied
//! into its bundle, laid out the way its agent reads them. When a session opens, the bundle and
//! the MCP servers are handed over through the session's own parameters. Nothing here writes an
//! agent's configuration.
// @project-doc docs/domains/20261116-resource_library.md#delivery

use super::{Body, Item, ItemKind, McpServer, McpValues, place_skill};
use crate::agents::Runtime;
use anyhow::Result;
use serde::Serialize;
use serde_json::{Value, json};
use std::path::Path;

/// Whether a runtime can use an item at all; the app shows it beside each choice.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "state", content = "why", rename_all = "lowercase")]
pub enum Support {
    Yes,
    No(&'static str),
    /// Decided by what the agent declares when it starts.
    Depends(&'static str),
}

pub fn support(runtime: Runtime, body: &Body) -> Support {
    match (runtime, body) {
        (Runtime::Custom, Body::Skill) => Support::No("自定义命令的 Agent 没有在启动时加载 Skill 的入口"),
        (_, Body::Skill) | (_, Body::Mcp { server: McpServer::Stdio { .. } }) | (Runtime::Claude, _) => Support::Yes,
        (Runtime::Codex, Body::Mcp { server: McpServer::Http { .. } }) => Support::Yes,
        (Runtime::Codex, Body::Mcp { server: McpServer::Sse { .. } }) => {
            Support::No("Codex 不支持 SSE 传输的 MCP 服务器")
        }
        (Runtime::Custom, Body::Mcp { server: McpServer::Http { .. } }) => {
            Support::Depends("要看 Agent 是否声明 HTTP MCP 能力")
        }
        (Runtime::Custom, Body::Mcp { server: McpServer::Sse { .. } }) => {
            Support::Depends("要看 Agent 是否声明 SSE MCP 能力")
        }
    }
}

/// Where a runtime reads a bundle's skills, relative to the bundle: Claude Code loads the bundle
/// as a local plugin, Codex reads `.agents/skills` under each additional root.
fn skills_dir(runtime: Runtime) -> Option<&'static str> {
    match runtime {
        Runtime::Claude => Some("skills"),
        Runtime::Codex => Some(".agents/skills"),
        Runtime::Custom => None,
    }
}

fn skills(items: &[Item]) -> impl Iterator<Item = &Item> {
    items.iter().filter(|i| i.body == Body::Skill)
}

/// Copy a member's skills into its bundle. A member with no skill its agent can load gets none.
pub fn bundle(store: &Path, runtime: Runtime, items: &[Item], dir: &Path) -> Result<()> {
    let Some(rel) = skills_dir(runtime) else { return Ok(()) };
    if skills(items).next().is_none() {
        return Ok(());
    }
    for item in skills(items) {
        place_skill(store, item, &dir.join(rel).join(&item.name))?;
    }
    if runtime == Runtime::Claude {
        let manifest = dir.join(".claude-plugin").join("plugin.json");
        std::fs::create_dir_all(manifest.parent().unwrap())?;
        let plugin = json!({ "name": "janus", "description": "Janus 资源库中这个角色选择的 Skill" });
        std::fs::write(manifest, serde_json::to_vec_pretty(&plugin)?)?;
    }
    Ok(())
}

/// How one resource reached a session, or why it did not.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "via", content = "why", rename_all = "camelCase")]
pub enum Via {
    /// Claude Code: the bundle as a local plugin (`_meta.claudeCode.options.plugins`).
    Plugin,
    /// Codex: the bundle as an additional root (`_meta.additionalRoots`).
    AdditionalRoots,
    McpServers,
    Skipped(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Delivery {
    pub item: String,
    pub name: String,
    pub kind: ItemKind,
    pub revision: String,
    #[serde(flatten)]
    pub via: Via,
}

/// What a session opens with for a member's resources.
pub struct Launch {
    /// Entries for `session/new` / `session/load` `mcpServers`.
    pub mcp_servers: Vec<Value>,
    /// The `_meta` that points the agent at the bundle's skills.
    pub meta: Value,
    pub delivered: Vec<Delivery>,
}

/// The resources of a session of `runtime`, given the capabilities its agent declared in
/// `initialize`. `values` reads a server's saved values when the session opens, so a value
/// changed since the run started reaches it; values never enter the run's record.
pub fn launch(
    runtime: Runtime,
    items: &[Item],
    caps: &Value,
    bundle: &Path,
    values: impl Fn(&str) -> McpValues,
) -> Launch {
    let mut out = Launch { mcp_servers: vec![], meta: json!({}), delivered: vec![] };
    let record = |item: &Item, via: Via| Delivery {
        item: item.id.clone(),
        name: item.name.clone(),
        kind: item.kind(),
        revision: item.revision.clone(),
        via,
    };
    let skill_via = match runtime {
        Runtime::Claude => Via::Plugin,
        Runtime::Codex => Via::AdditionalRoots,
        Runtime::Custom => Via::Skipped("自定义命令的 Agent 没有在启动时加载 Skill 的入口".into()),
    };
    if skills(items).next().is_some() {
        let path = bundle.to_string_lossy();
        match runtime {
            Runtime::Claude => {
                out.meta = json!({ "claudeCode": { "options": { "plugins": [{ "type": "local", "path": path }] } } })
            }
            Runtime::Codex => out.meta = json!({ "additionalRoots": [path] }),
            Runtime::Custom => {}
        }
    }
    let mcp = &caps["mcpCapabilities"];
    for item in items {
        match &item.body {
            Body::Skill => out.delivered.push(record(item, skill_via.clone())),
            Body::Mcp { server } => {
                let missing = match server {
                    McpServer::Stdio { .. } => None,
                    McpServer::Http { .. } => (mcp["http"] != true).then_some("Agent 没有声明 HTTP MCP 能力"),
                    McpServer::Sse { .. } => (mcp["sse"] != true).then_some("Agent 没有声明 SSE MCP 能力"),
                };
                match missing {
                    Some(why) => out.delivered.push(record(item, Via::Skipped(why.into()))),
                    None => {
                        out.mcp_servers.push(server.acp(&item.name, &values(&item.id)));
                        out.delivered.push(record(item, Via::McpServers));
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::Origin;
    use std::collections::BTreeMap;

    fn item(id: &str, name: &str, body: Body) -> Item {
        Item {
            id: id.into(),
            name: name.into(),
            description: "d".into(),
            origin: Origin::Created,
            revision: format!("rev-{id}"),
            updated_at: 0,
            body,
        }
    }

    fn items() -> Vec<Item> {
        let stdio = McpServer::Stdio { command: "node".into(), args: vec![], env_keys: vec!["TOKEN".into()] };
        let sse = McpServer::Sse { url: "https://e.example/sse".into(), header_keys: vec![] };
        vec![
            item("i1", "pdf", Body::Skill),
            item("i2", "github", Body::Mcp { server: stdio }),
            item("i3", "events", Body::Mcp { server: sse }),
        ]
    }

    fn values(id: &str) -> McpValues {
        let env = if id == "i2" { BTreeMap::from([("TOKEN".to_string(), "t0k".to_string())]) } else { BTreeMap::new() };
        McpValues { env, ..Default::default() }
    }

    /// Claude Code gets the bundle as a local plugin and every server it declares support for.
    #[test]
    fn claude_loads_the_bundle_as_a_plugin() {
        let caps = json!({ "mcpCapabilities": { "http": true, "sse": true } });
        let l = launch(Runtime::Claude, &items(), &caps, Path::new("/runs/r1/members/lead/bundle"), values);
        assert_eq!(
            l.meta,
            json!({ "claudeCode": { "options": { "plugins": [{ "type": "local", "path": "/runs/r1/members/lead/bundle" }] } } })
        );
        assert_eq!(l.mcp_servers.len(), 2);
        assert_eq!(l.mcp_servers[0]["env"], json!([{ "name": "TOKEN", "value": "t0k" }]));
        assert_eq!(l.delivered[0].via, Via::Plugin);
        let record = serde_json::to_string(&l.delivered).unwrap();
        assert!(!record.contains("t0k"), "values stay out of the record: {record}");
    }

    /// Codex reads the bundle's `.agents/skills` as an additional root and has no SSE servers.
    #[test]
    fn codex_reads_the_bundle_as_an_additional_root() {
        let caps = json!({ "mcpCapabilities": { "http": true, "sse": false } });
        let l = launch(Runtime::Codex, &items(), &caps, Path::new("/b"), values);
        assert_eq!(l.meta, json!({ "additionalRoots": ["/b"] }));
        assert_eq!(l.mcp_servers.len(), 1);
        assert_eq!(l.delivered[1].via, Via::McpServers);
        assert!(matches!(l.delivered[2].via, Via::Skipped(_)));
        assert_eq!(support(Runtime::Codex, &items()[2].body), Support::No("Codex 不支持 SSE 传输的 MCP 服务器"));
    }

    /// A custom agent gets the servers it can reach and no skills.
    #[test]
    fn custom_agents_get_servers_only() {
        let l = launch(Runtime::Custom, &items(), &json!({}), Path::new("/b"), values);
        assert_eq!(l.meta, json!({}));
        assert_eq!(l.mcp_servers.len(), 1);
        assert!(matches!(l.delivered[0].via, Via::Skipped(_)));
    }

    /// The bundle is laid out per runtime, with the library's name in each SKILL.md.
    #[test]
    fn bundles_follow_the_runtime() {
        let root = std::env::temp_dir().join(format!("janus-bundle-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let store = root.join("store");
        let src = root.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("SKILL.md"), "---\nname: original\ndescription: d\n---\nBody").unwrap();
        let mut skill = item("i1", "pdf", Body::Skill);
        skill.revision = super::super::store::put(&store, &src).unwrap();
        let claude = root.join("claude");
        bundle(&store, Runtime::Claude, std::slice::from_ref(&skill), &claude).unwrap();
        assert!(claude.join(".claude-plugin/plugin.json").is_file());
        let md = std::fs::read_to_string(claude.join("skills/pdf/SKILL.md")).unwrap();
        assert!(md.starts_with("---\nname: pdf\n"), "{md}");
        let codex = root.join("codex");
        bundle(&store, Runtime::Codex, std::slice::from_ref(&skill), &codex).unwrap();
        assert!(codex.join(".agents/skills/pdf/SKILL.md").is_file());
        let custom = root.join("custom");
        bundle(&store, Runtime::Custom, &[skill], &custom).unwrap();
        assert!(!custom.exists());
        // What an agent does to its copy stays in the bundle.
        std::fs::write(codex.join(".agents/skills/pdf/SKILL.md"), "changed").unwrap();
        assert!(
            std::fs::read_to_string(
                store
                    .read_dir()
                    .unwrap()
                    .flatten()
                    .find(|e| !e.file_name().to_string_lossy().starts_with('.'))
                    .unwrap()
                    .path()
                    .join("SKILL.md")
            )
            .unwrap()
            .contains("original")
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
