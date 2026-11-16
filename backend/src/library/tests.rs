//! The library through `App`: what each operation keeps, refuses and leaves behind.

use super::*;
use crate::orchestrator::test_app;
use std::sync::Arc;

fn app(name: &str) -> (Arc<App>, PathBuf) {
    let dir = std::env::temp_dir().join(format!("janus-library-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    (test_app(&dir), dir)
}

fn sh(dir: &Path, args: &[&str]) {
    let ok = std::process::Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

fn skill_folder(dir: &Path, name: &str, body: &str) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("SKILL.md"), format!("---\nname: {name}\ndescription: does {name}\n---\n{body}\n"))
        .unwrap();
}

/// A skill written in Janus: stored by content, named for every agent, edited into a new revision.
#[tokio::test]
async fn editor_skills_are_stored_and_edited() {
    let (app, dir) = app("editor");
    let item = app
        .create_skill(&json!({ "name": "Release Notes", "description": "写发布说明", "content": "按模板写。" }))
        .await
        .unwrap();
    assert_eq!(item.name, "release-notes");
    assert_eq!(item.origin, Origin::Created);
    let text = app.skill_text(&item).unwrap();
    assert!(text.starts_with("---\nname: release-notes\ndescription: \"写发布说明\"\n---"), "{text}");
    let dup = app.create_skill(&json!({ "name": "release-notes", "description": "d", "content": "x" })).await;
    assert!(dup.unwrap_err().to_string().contains("已经有"));
    assert!(app.create_skill(&json!({ "name": "x", "content": "no description" })).await.is_err());

    let edited = app
        .edit_skill(&item.id, &json!({ "name": "release-notes", "content": text.replace("按模板写", "按新模板写") }))
        .await
        .unwrap();
    assert_ne!(edited.revision, item.revision);
    assert!(!app.store_dir().join(&item.revision).exists(), "the old tree is swept");
    assert_eq!(edited.description, "写发布说明");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A folder is copied once; refreshing copies it again. Only editor skills are edited in place.
#[tokio::test]
async fn folder_skills_are_copied_and_refreshed() {
    let (app, dir) = app("folder");
    let src = dir.join("mine/PDF Tools");
    skill_folder(&src, "PDF Tools", "v1");
    let item = app.import_skill_folder(&src.to_string_lossy()).await.unwrap();
    assert_eq!(item.name, "pdf-tools");
    let again = app.import_skill_folder(&src.to_string_lossy()).await.unwrap();
    assert_eq!(again.name, "pdf-tools-2", "a taken name gets a suffix");
    assert_eq!(again.revision, item.revision, "one stored tree for the same content");
    skill_folder(&src, "PDF Tools", "v2");
    let fresh = app.refresh_item(&item.id).await.unwrap();
    assert_ne!(fresh.revision, item.revision);
    assert!(app.edit_skill(&item.id, &json!({ "name": "x", "content": "y" })).await.is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Server values live in their own 0600 file, never in the item, the view or the database.
#[tokio::test]
async fn mcp_values_stay_out_of_records() {
    let (app, dir) = app("mcp");
    let body = json!({
        "name": "github", "transport": "stdio", "command": "npx -y gh-mcp",
        "vars": [{ "name": "GITHUB_TOKEN", "value": "ghp-secret" }, { "name": "UNSET", "value": "" }]
    });
    let item = app.save_mcp(None, &body).await.unwrap();
    assert_eq!(
        item.body,
        Body::Mcp {
            server: McpServer::Stdio {
                command: "npx".into(),
                args: vec!["-y".into(), "gh-mcp".into()],
                env_keys: vec!["GITHUB_TOKEN".into(), "UNSET".into()]
            }
        }
    );
    assert_eq!(app.mcp_values(&item.id).env["GITHUB_TOKEN"], "ghp-secret");
    let view = app.library_view().unwrap().to_string();
    assert!(!view.contains("ghp-secret") && view.contains("\"set\":true"), "{view}");
    let db = std::fs::read(dir.join("janus.db")).unwrap();
    assert!(!String::from_utf8_lossy(&db).contains("ghp-secret"));
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(app.values_path(&item.id).unwrap()).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);

    // An empty value keeps the saved one; a key no longer listed loses its value.
    let keep = json!({ "name": "github", "transport": "stdio", "command": "npx -y gh-mcp", "vars": [{ "name": "GITHUB_TOKEN", "value": "" }] });
    app.save_mcp(Some(&item.id), &keep).await.unwrap();
    assert_eq!(app.mcp_values(&item.id).env.get("GITHUB_TOKEN").map(String::as_str), Some("ghp-secret"));
    assert!(app.save_mcp(None, &json!({ "name": "janus", "command": "x" })).await.is_err(), "the board's name");
    assert!(app.save_mcp(None, &json!({ "name": "github", "command": "x" })).await.is_err(), "names are unique");

    let pasted = app
        .import_mcp_config(r#"{"mcpServers": {"github": {"command": "npx"}, "docs": {"type": "sse", "url": "https://d.example/sse", "headers": {"Authorization": "Bearer h"}}}}"#)
        .await
        .unwrap();
    let names: Vec<&str> = pasted["items"].as_array().unwrap().iter().map(|i| i["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["docs", "github-2"]);
    let docs = pasted["items"][0]["id"].as_str().unwrap();
    assert_eq!(app.mcp_values(docs).headers["Authorization"], "Bearer h");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A role that uses an item keeps it from being deleted; deleting takes its values along.
#[tokio::test]
async fn items_in_use_are_kept() {
    let (app, dir) = app("delete");
    let item = app
        .save_mcp(None, &json!({ "name": "fs", "command": "mcp-fs", "vars": [{ "name": "K", "value": "v" }] }))
        .await
        .unwrap();
    app.db.put_config(ROLE, "r1", &json!({ "id": "r1", "name": "研发", "resources": [item.id] }), now_ms()).unwrap();
    let err = app.delete_item(&item.id).await.unwrap_err().to_string();
    assert!(err.contains("研发"), "{err}");
    app.db.put_config(ROLE, "r1", &json!({ "id": "r1", "name": "研发", "resources": [] }), now_ms()).unwrap();
    app.delete_item(&item.id).await.unwrap();
    assert!(!app.values_path(&item.id).unwrap().exists());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A Git source lists what it offers; imported items follow it on update and stay when it drops them.
#[tokio::test]
async fn git_sources_offer_import_and_update() {
    let (app, dir) = app("git");
    let upstream = dir.join("upstream");
    skill_folder(&upstream.join("skills/pdf"), "pdf", "v1");
    skill_folder(&upstream.join(".claude/skills/review"), "review", "v1");
    skill_folder(&upstream.join("skills/pdf/examples/inner"), "inner", "x"); // part of pdf
    std::fs::create_dir_all(upstream.join("node_modules/x")).unwrap();
    std::fs::write(upstream.join("node_modules/x/SKILL.md"), "no").unwrap();
    std::os::unix::fs::symlink(upstream.join("skills"), upstream.join("linked")).unwrap();
    std::fs::write(
        upstream.join("mcp.json"),
        r#"{"mcpServers": {"search": {"command": "search-mcp", "env": {"SEARCH_KEY": "from-manifest"}}}}"#,
    )
    .unwrap();
    sh(&upstream, &["init", "-q"]);
    sh(&upstream, &["add", "-A"]);
    sh(&upstream, &["commit", "-q", "-m", "init"]);

    let url = format!("file://{}", upstream.display());
    let at = || sources::RepoAt { repo: "acme/kit".into(), git_ref: String::new(), path: String::new() };
    let src = app.add_source_at(at(), &url).await.unwrap();
    let id = src["id"].as_str().unwrap().to_string();
    let offered: Vec<&str> = src["offers"].as_array().unwrap().iter().map(|o| o["path"].as_str().unwrap()).collect();
    assert_eq!(
        offered,
        [".claude/skills/review", "skills/pdf", "mcp.json#search"],
        "no links, node_modules or nested folders"
    );
    assert!(app.add_source_at(at(), &url).await.is_err(), "the same source twice");
    assert!(app.items().unwrap().is_empty(), "nothing is imported until picked");

    let added = app.import_from_source(&id, &["skills/pdf".into(), "mcp.json#search".into()]).await.unwrap();
    let (pdf, search) = (&added[0], &added[1]);
    assert_eq!(pdf.origin, Origin::Git { source: id.clone(), path: "skills/pdf".into() });
    assert_eq!(app.mcp_values(&search.id).env["SEARCH_KEY"], "from-manifest");
    assert!(app.delete_source(&id).await.unwrap_err().to_string().contains("2 项"));

    // The person sets their own key; upstream changes pdf, drops the server and adds a skill.
    app.save_mcp(
        Some(&search.id),
        &json!({ "name": "search", "command": "search-mcp", "vars": [{ "name": "SEARCH_KEY", "value": "mine" }] }),
    )
    .await
    .unwrap();
    skill_folder(&upstream.join("skills/pdf"), "pdf", "v2");
    std::fs::remove_file(upstream.join("mcp.json")).unwrap();
    skill_folder(&upstream.join("skills/lint"), "lint", "v1");
    sh(&upstream, &["add", "-A"]);
    sh(&upstream, &["commit", "-q", "-m", "next"]);
    let checked = app.check_sources().await.unwrap();
    assert_eq!(checked["sources"][0]["updateAvailable"], true, "{checked}");

    let done = app.update_source(&id).await.unwrap();
    assert_eq!(done["updated"], json!(["pdf"]));
    assert_eq!(done["missing"], json!(["search"]));
    assert_eq!(done["source"]["updateAvailable"], false);
    assert_ne!(app.item(&pdf.id).unwrap().revision, pdf.revision);
    assert_eq!(app.mcp_values(&search.id).env["SEARCH_KEY"], "mine", "saved values survive");
    let view = app.library_view().unwrap();
    let gone = view["items"].as_array().unwrap().iter().find(|i| i["id"] == search.id.as_str()).unwrap();
    assert_eq!(gone["source"]["inSource"], false);

    app.delete_item(&pdf.id).await.unwrap();
    app.delete_item(&search.id).await.unwrap();
    app.delete_source(&id).await.unwrap();
    assert!(!dir.join("data/library/sources").join(&id).exists());
    assert!(app.store_dir().read_dir().map(|d| d.count()).unwrap_or(0) == 0, "no tree is left behind");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Items serialize with their kind beside the other fields, the way the app and run records read them.
#[test]
fn items_read_back_as_written() {
    let item = Item {
        id: "i1".into(),
        name: "docs".into(),
        description: String::new(),
        origin: Origin::Git { source: "s1".into(), path: "mcp.json#docs".into() },
        revision: "r".into(),
        updated_at: 1,
        body: Body::Mcp { server: McpServer::Http { url: "https://d/mcp".into(), header_keys: vec![] } },
    };
    let v = serde_json::to_value(&item).unwrap();
    assert_eq!(v["kind"], "mcp");
    assert_eq!(v["origin"], json!({ "type": "git", "source": "s1", "path": "mcp.json#docs" }));
    assert_eq!(v["server"]["transport"], "http");
    assert_eq!(serde_json::from_value::<Item>(v).unwrap(), item);
    let skill: Item = serde_json::from_value(json!({
        "id": "i2", "name": "pdf", "origin": { "type": "created" }, "revision": "h", "updatedAt": 0, "kind": "skill"
    }))
    .unwrap();
    assert_eq!(skill.kind(), ItemKind::Skill);
}
