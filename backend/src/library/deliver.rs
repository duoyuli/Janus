//! How a member's resources reach its sessions: only through Janus's two routers. When a run
//! starts, the member's skills are copied into its bundle behind one skill, `janus-skills`, laid
//! out the way its agent reads skills. When a session opens, the bundle is handed over through the
//! session's own parameters, and the MCP servers stay behind the run's MCP router, `janus-tools`.
//! Nothing here writes an agent's configuration.
// @project-doc docs/domains/20261116-resource_library.md#delivery

use super::{Body, Item, ItemKind, place_skill};
use crate::agents::Runtime;
use anyhow::Result;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Where a runtime reads a bundle's skills, relative to the bundle: Claude Code loads the bundle
/// as a local plugin, Codex reads `.agents/skills` under each additional root.
fn skills_dir(runtime: Runtime) -> &'static str {
    match runtime {
        Runtime::Claude => "skills",
        Runtime::Codex => ".agents/skills",
    }
}

fn skills(items: &[Item]) -> impl Iterator<Item = &Item> {
    items.iter().filter(|i| i.body == Body::Skill)
}

/// The name of the skill that lists the member's skills, and of the MCP router; neither can be an
/// item's name.
pub const SKILL_ROUTER: &str = "janus-skills";
pub const MCP_ROUTER: &str = "janus-tools";

/// Copy a member's skills into its bundle, under `janus-skills/.skills/`, hidden from the agent's
/// own scan; `janus-skills` is the catalog of them (`skill_router`). The session's prompt so gets
/// one description however many skills the library holds. A member with no skill gets no bundle.
// @project-doc docs/domains/20261116-resource_library.md#skill_router
pub fn bundle(store: &Path, runtime: Runtime, items: &[Item], uses: &HashMap<String, u64>, dir: &Path) -> Result<()> {
    let routed: Vec<&Item> = skills(items).collect();
    if routed.is_empty() {
        return Ok(());
    }
    let router = dir.join(skills_dir(runtime)).join(SKILL_ROUTER);
    for item in &routed {
        place_skill(store, item, &router.join(".skills").join(&item.name))?;
    }
    super::skill_router::write(&router, &routed, uses)?;
    if runtime == Runtime::Claude {
        let manifest = dir.join(".claude-plugin").join("plugin.json");
        std::fs::create_dir_all(manifest.parent().unwrap())?;
        let plugin = json!({ "name": "janus", "description": "Janus 资源库的 Skill" });
        std::fs::write(manifest, serde_json::to_vec_pretty(&plugin)?)?;
    }
    Ok(())
}

/// The router's SKILL.md in a member's bundle, when the member has skills. Janus's instructions
/// for the session name it, so the agent finds the library's skills even when its own skill
/// listing has no room left for the router's description.
pub fn skill_router_md(runtime: Runtime, items: &[Item], bundle: &Path) -> Option<PathBuf> {
    skills(items).next()?;
    Some(bundle.join(skills_dir(runtime)).join(SKILL_ROUTER).join("SKILL.md"))
}

/// How one resource reached a session, or why it did not.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "via", content = "why", rename_all = "camelCase")]
pub enum Via {
    /// Listed by `janus-skills` in the bundle, read when a task needs it.
    SkillRouter,
    /// Served through the run's MCP router (`janus-tools`).
    McpRouter,
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
    /// Entries for `session/new` / `session/load` `mcpServers`: the router's, or none.
    pub mcp_servers: Vec<Value>,
    /// The `_meta` that points the agent at the bundle: Claude Code loads it as a local plugin
    /// (`_meta.claudeCode.options.plugins`), Codex as an additional root (`_meta.additionalRoots`).
    pub meta: Value,
    pub delivered: Vec<Delivery>,
}

/// The resources of a session of `runtime`, given the capabilities its agent declared in
/// `initialize`. `router` is the run's MCP router entry for this session: every MCP server of the
/// member goes through it, so the session gets that one entry whatever the library holds. An agent
/// that takes no HTTP MCP gets no router and so no server.
pub fn launch(runtime: Runtime, items: &[Item], caps: &Value, bundle: &Path, router: Value) -> Launch {
    let mut out = Launch { mcp_servers: vec![], meta: json!({}), delivered: vec![] };
    if skills(items).next().is_some() {
        let path = bundle.to_string_lossy();
        out.meta = match runtime {
            Runtime::Claude => {
                json!({ "claudeCode": { "options": { "plugins": [{ "type": "local", "path": path }] } } })
            }
            Runtime::Codex => json!({ "additionalRoots": [path] }),
        };
    }
    let http = caps["mcpCapabilities"]["http"] == true;
    let mut router_used = false;
    for item in items {
        let via = match &item.body {
            Body::Skill => Via::SkillRouter,
            Body::Mcp { .. } if http => {
                router_used = true;
                Via::McpRouter
            }
            Body::Mcp { .. } => Via::Skipped("Agent 没有声明 HTTP MCP 能力，拿不到 MCP 路由".into()),
        };
        out.delivered.push(Delivery {
            item: item.id.clone(),
            name: item.name.clone(),
            kind: item.kind(),
            revision: item.revision.clone(),
            via,
        });
    }
    if router_used {
        out.mcp_servers.push(router);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::{McpProcess, McpServer, Origin};

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
        let stdio = McpServer::Stdio {
            command: "node".into(),
            args: vec![],
            env_keys: vec!["TOKEN".into()],
            process: McpProcess::PerRun,
        };
        let http = McpServer::Http { url: "https://e.example/mcp".into(), header_keys: vec!["Authorization".into()] };
        let sse = McpServer::Sse { url: "https://e.example/sse".into(), header_keys: vec![] };
        vec![
            item("i1", "pdf", Body::Skill),
            item("i2", "github", Body::Mcp { server: stdio }),
            item("i3", "infra", Body::Mcp { server: http }),
            item("i4", "events", Body::Mcp { server: sse }),
        ]
    }

    fn router() -> Value {
        json!({ "type": "http", "name": MCP_ROUTER, "url": "http://h/mcp/r/t/T1/tools", "headers": [] })
    }

    /// Every MCP server, whatever its transport, goes through the router:
    /// the session gets the router's one entry and no server of its own, so its tool list does not
    /// grow with the library. Values never reach the session or the record.
    #[test]
    fn every_server_goes_through_the_router() {
        let caps = json!({ "mcpCapabilities": { "http": true, "sse": false } });
        for runtime in [Runtime::Claude, Runtime::Codex] {
            let l = launch(runtime, &items(), &caps, Path::new("/b"), router());
            assert_eq!(l.mcp_servers, vec![router()], "{runtime:?}");
            assert!(l.delivered[1..].iter().all(|d| d.via == Via::McpRouter), "{:?}", l.delivered);
        }
        let skills_only = launch(Runtime::Claude, &items()[..1], &caps, Path::new("/b"), router());
        assert!(skills_only.mcp_servers.is_empty(), "no server, no router");
        let no_http = launch(Runtime::Claude, &items(), &json!({}), Path::new("/b"), router());
        assert!(no_http.mcp_servers.is_empty());
        assert!(matches!(no_http.delivered[2].via, Via::Skipped(_)));
    }

    /// The bundle reaches Claude Code as a local plugin and Codex as an additional root.
    #[test]
    fn the_bundle_follows_the_runtime() {
        let caps = json!({ "mcpCapabilities": { "http": true } });
        let claude = launch(Runtime::Claude, &items(), &caps, Path::new("/runs/r1/members/lead/bundle"), router());
        assert_eq!(
            claude.meta,
            json!({ "claudeCode": { "options": { "plugins": [{ "type": "local", "path": "/runs/r1/members/lead/bundle" }] } } })
        );
        assert_eq!(claude.delivered[0].via, Via::SkillRouter);
        let codex = launch(Runtime::Codex, &items(), &caps, Path::new("/b"), router());
        assert_eq!(codex.meta, json!({ "additionalRoots": ["/b"] }));
        let none = launch(Runtime::Codex, &items()[1..], &caps, Path::new("/b"), router());
        assert_eq!(none.meta, json!({}), "no skill, no bundle");
    }

    /// Every skill, one or many, sits under `janus-skills`' hidden
    /// `.skills`, so the agent loads only `janus-skills`, the catalog of them. The
    /// copy is the member's own: what an agent does to it stays in the bundle.
    #[test]
    fn skills_go_through_janus_skills() {
        let root = std::env::temp_dir().join(format!("janus-skill-router-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (store, src) = (root.join("store"), root.join("src"));
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("SKILL.md"), "---\nname: original\ndescription: d\n---\nBody").unwrap();
        let rev = super::super::store::put(&store, &src).unwrap();
        let skill = |name: &str| {
            let mut i = item(name, name, Body::Skill);
            i.revision = rev.clone();
            i
        };
        let list = vec![skill("pdf"), skill("docx")];
        let claude = root.join("claude");
        bundle(&store, Runtime::Claude, &list, &HashMap::new(), &claude).unwrap();
        assert!(claude.join(".claude-plugin/plugin.json").is_file());
        let router = std::fs::read_to_string(claude.join("skills/janus-skills/SKILL.md")).unwrap();
        assert!(router.starts_with("---\nname: janus-skills\n"), "{router}");
        assert!(router.contains("- pdf：d") && router.contains("- docx：d"), "{router}");
        let path = skill_router_md(Runtime::Claude, &list, &claude).unwrap();
        assert_eq!(path, claude.join("skills/janus-skills/SKILL.md"));
        assert_eq!(skill_router_md(Runtime::Claude, &items()[1..], &claude), None, "no skill, no router");
        let md = std::fs::read_to_string(claude.join("skills/janus-skills/.skills/pdf/SKILL.md")).unwrap();
        assert!(md.starts_with("---\nname: pdf\n"), "the library's name: {md}");
        assert!(!claude.join("skills/pdf").exists() && !claude.join("skills/docx").exists());
        let codex = root.join("codex");
        bundle(&store, Runtime::Codex, &list[..1], &HashMap::new(), &codex).unwrap();
        let copy = codex.join(".agents/skills/janus-skills/.skills/pdf/SKILL.md");
        assert!(copy.is_file());
        std::fs::write(&copy, "changed").unwrap();
        assert!(std::fs::read_to_string(store.join(&rev).join("SKILL.md")).unwrap().contains("original"));
        let empty = root.join("empty");
        bundle(&store, Runtime::Codex, &items()[1..], &HashMap::new(), &empty).unwrap();
        assert!(!empty.exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
