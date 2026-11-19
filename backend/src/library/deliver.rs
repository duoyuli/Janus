//! How a member's resources reach its sessions. When a run starts, the member's skills are copied
//! into its bundle, laid out the way its agent reads them. When a session opens, the bundle and
//! the MCP servers are handed over through the session's own parameters. Nothing here writes an
//! agent's configuration.
// @project-doc docs/domains/20261116-resource_library.md#delivery

use super::{Body, Item, ItemKind, McpServer, McpValues, Reach, place_skill};
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

/// Past this many skills for everyone, they reach a session through one skill, `janus-skills`,
/// instead of each with its own description in the prompt.
pub const SKILL_ROUTE_OVER: usize = 10;

/// The name of the skill that lists the routed skills, and of the MCP router; neither can be an
/// item's name.
pub const SKILL_ROUTER: &str = "janus-skills";
pub const MCP_ROUTER: &str = "janus-tools";

/// The skills for everyone that go through `janus-skills`: all of them past `SKILL_ROUTE_OVER`,
/// none otherwise.
fn routed_skills(items: &[Item]) -> Vec<&Item> {
    let everyone: Vec<&Item> = skills(items).filter(|i| i.reach == Reach::Everyone).collect();
    if everyone.len() > SKILL_ROUTE_OVER { everyone } else { vec![] }
}

/// Whether a server for everyone goes through the run's MCP router: a command started on this
/// machine. A URL starts nothing here, so it is handed over as it is. An agent that takes no
/// HTTP MCP gets no router (`launch`), so each server directly.
pub fn routed_server(item: &Item) -> bool {
    item.reach == Reach::Everyone && matches!(item.body, Body::Mcp { server: McpServer::Stdio { .. } })
}

/// Copy a member's skills into its bundle. A member with no skill its agent can load gets none.
/// Skills for everyone past `SKILL_ROUTE_OVER` go under `janus-skills/.skills/`, hidden from the
/// agent's own scan, and `janus-skills/SKILL.md` lists each with its description and path.
// @project-doc docs/domains/20261116-resource_library.md#skill_router
pub fn bundle(store: &Path, runtime: Runtime, items: &[Item], dir: &Path) -> Result<()> {
    let Some(rel) = skills_dir(runtime) else { return Ok(()) };
    if skills(items).next().is_none() {
        return Ok(());
    }
    let routed = routed_skills(items);
    let router = dir.join(rel).join(SKILL_ROUTER);
    for item in skills(items) {
        let at = if routed.iter().any(|r| r.id == item.id) {
            router.join(".skills").join(&item.name)
        } else {
            dir.join(rel).join(&item.name)
        };
        place_skill(store, item, &at)?;
    }
    if !routed.is_empty() {
        std::fs::write(router.join("SKILL.md"), router_skill(&routed, &router))?;
    }
    if runtime == Runtime::Claude {
        let manifest = dir.join(".claude-plugin").join("plugin.json");
        std::fs::create_dir_all(manifest.parent().unwrap())?;
        let plugin = json!({ "name": "janus", "description": "Janus 资源库中这个角色选择的 Skill" });
        std::fs::write(manifest, serde_json::to_vec_pretty(&plugin)?)?;
    }
    Ok(())
}

/// `janus-skills`' SKILL.md: a description naming the skills (within the 1024 bytes Codex takes),
/// then each skill's whole description and the path of its SKILL.md.
fn router_skill(routed: &[&Item], router: &Path) -> String {
    let mut desc = String::from("Janus 资源库中给所有数字员工的 Skill，任务用得上时读对应的 SKILL.md：");
    for (i, item) in routed.iter().enumerate() {
        let next = format!("{}{}", if i == 0 { "" } else { "、" }, item.name);
        if desc.len() + next.len() > 1000 {
            desc.push_str(&format!(" 等 {} 个", routed.len()));
            break;
        }
        desc.push_str(&next);
    }
    let mut body = format!(
        "---\nname: {SKILL_ROUTER}\ndescription: {}\n---\n\n# Janus 资源库的 Skill\n\n下面每个 Skill 都可以用。任务和某个 Skill 的描述相符时，先读它的 SKILL.md（路径如下），再照着做；它提到的其他文件和 SKILL.md 在同一个目录。\n",
        desc.replace('\n', " ")
    );
    for item in routed {
        body.push_str(&format!(
            "\n## {}\n\n{}\n\n路径：{}\n",
            item.name,
            item.description,
            router.join(".skills").join(&item.name).join("SKILL.md").display()
        ));
    }
    body
}

/// How one resource reached a session, or why it did not.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "via", content = "why", rename_all = "camelCase")]
pub enum Via {
    /// Claude Code: the bundle as a local plugin (`_meta.claudeCode.options.plugins`).
    Plugin,
    /// Codex: the bundle as an additional root (`_meta.additionalRoots`).
    AdditionalRoots,
    /// Listed by `janus-skills` in the bundle, read when a task needs it.
    SkillRouter,
    McpServers,
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
    /// Entries for `session/new` / `session/load` `mcpServers`.
    pub mcp_servers: Vec<Value>,
    /// The `_meta` that points the agent at the bundle's skills.
    pub meta: Value,
    pub delivered: Vec<Delivery>,
}

/// The resources of a session of `runtime`, given the capabilities its agent declared in
/// `initialize`. `values` reads a server's saved values when the session opens, so a value
/// changed since the run started reaches it; values never enter the run's record.
/// `router` is the run's MCP router entry for this session; the servers for everyone that it
/// serves (`routed_server`) are not handed over one by one.
pub fn launch(
    runtime: Runtime,
    items: &[Item],
    caps: &Value,
    bundle: &Path,
    router: Option<Value>,
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
    let routed = routed_skills(items);
    let router = router.filter(|_| mcp["http"] == true);
    let mut router_used = false;
    for item in items {
        match &item.body {
            Body::Skill if routed.iter().any(|r| r.id == item.id) && runtime != Runtime::Custom => {
                out.delivered.push(record(item, Via::SkillRouter))
            }
            Body::Skill => out.delivered.push(record(item, skill_via.clone())),
            Body::Mcp { .. } if router.is_some() && routed_server(item) => {
                router_used = true;
                out.delivered.push(record(item, Via::McpRouter));
            }
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
    if let Some(entry) = router.filter(|_| router_used) {
        out.mcp_servers.push(entry);
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
            reach: crate::library::Reach::Picked,
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
            process: super::super::McpProcess::PerRun,
        };
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
        let l = launch(Runtime::Claude, &items(), &caps, Path::new("/runs/r1/members/lead/bundle"), None, values);
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
        let l = launch(Runtime::Codex, &items(), &caps, Path::new("/b"), None, values);
        assert_eq!(l.meta, json!({ "additionalRoots": ["/b"] }));
        assert_eq!(l.mcp_servers.len(), 1);
        assert_eq!(l.delivered[1].via, Via::McpServers);
        assert!(matches!(l.delivered[2].via, Via::Skipped(_)));
        assert_eq!(support(Runtime::Codex, &items()[2].body), Support::No("Codex 不支持 SSE 传输的 MCP 服务器"));
    }

    /// A custom agent gets the servers it can reach and no skills.
    #[test]
    fn custom_agents_get_servers_only() {
        let l = launch(Runtime::Custom, &items(), &json!({}), Path::new("/b"), None, values);
        assert_eq!(l.meta, json!({}));
        assert_eq!(l.mcp_servers.len(), 1);
        assert!(matches!(l.delivered[0].via, Via::Skipped(_)));
    }

    /// A command server for everyone goes through the run's router: no entry of its own, the
    /// router's once. One kept for some, and a URL, are handed over as they are; an agent that
    /// takes no HTTP MCP gets no router and so each server directly.
    #[test]
    fn servers_for_everyone_go_through_the_router() {
        let mut list = items();
        list[1].reach = Reach::Everyone;
        let router = json!({ "type": "http", "name": MCP_ROUTER, "url": "http://h/mcp/r/t/T1/tools", "headers": [] });
        let caps = json!({ "mcpCapabilities": { "http": true, "sse": true } });
        let l = launch(Runtime::Claude, &list, &caps, Path::new("/b"), Some(router.clone()), values);
        assert_eq!(l.mcp_servers, vec![l.mcp_servers[0].clone(), router.clone()]);
        assert_eq!(l.mcp_servers[0]["name"], "events", "the SSE server is not routed");
        assert_eq!(l.delivered[1].via, Via::McpRouter);
        let kept = launch(Runtime::Claude, &items(), &caps, Path::new("/b"), Some(router.clone()), values);
        assert!(!kept.mcp_servers.contains(&router), "no router without a routed server");
        let no_http = launch(Runtime::Claude, &list, &json!({}), Path::new("/b"), Some(router), values);
        assert_eq!(no_http.mcp_servers[0]["name"], "github");
    }

    /// Past ten skills for everyone, one `janus-skills` lists them; they sit under its hidden
    /// `.skills` so the agent does not load each. A skill kept for some stays a skill of its own.
    #[test]
    fn many_skills_for_everyone_go_through_janus_skills() {
        let root = std::env::temp_dir().join(format!("janus-skill-router-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (store, src) = (root.join("store"), root.join("src"));
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("SKILL.md"), "---\nname: x\ndescription: d\n---\nBody").unwrap();
        let rev = super::super::store::put(&store, &src).unwrap();
        let skill = |n: usize, reach: Reach| {
            let mut i = item(&format!("s{n}"), &format!("skill-{n}"), Body::Skill);
            (i.revision, i.reach) = (rev.clone(), reach);
            i
        };
        let mut list: Vec<Item> = (0..=SKILL_ROUTE_OVER).map(|n| skill(n, Reach::Everyone)).collect();
        list.push(skill(99, Reach::Picked));
        let dir = root.join("claude");
        bundle(&store, Runtime::Claude, &list, &dir).unwrap();
        let router = std::fs::read_to_string(dir.join("skills/janus-skills/SKILL.md")).unwrap();
        assert!(router.starts_with("---\nname: janus-skills\n"), "{router}");
        assert!(router.contains("## skill-0") && router.contains(".skills/skill-0/SKILL.md"), "{router}");
        assert!(dir.join("skills/janus-skills/.skills/skill-10/SKILL.md").is_file());
        assert!(!dir.join("skills/skill-0").exists(), "a routed skill is not loaded on its own");
        assert!(dir.join("skills/skill-99/SKILL.md").is_file(), "a skill kept for some stays direct");
        let l = launch(Runtime::Claude, &list, &json!({}), &dir, None, values);
        assert_eq!(l.delivered[0].via, Via::SkillRouter);
        assert_eq!(l.delivered.last().unwrap().via, Via::Plugin);
        // At ten or fewer they are each a skill of their own.
        let few: Vec<Item> = (0..SKILL_ROUTE_OVER).map(|n| skill(n, Reach::Everyone)).collect();
        let dir = root.join("few");
        bundle(&store, Runtime::Claude, &few, &dir).unwrap();
        assert!(dir.join("skills/skill-0/SKILL.md").is_file() && !dir.join("skills/janus-skills").exists());
        let _ = std::fs::remove_dir_all(&root);
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
