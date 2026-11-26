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

/// A script `run_code` saved is a skill written in Janus with its `code.js`, which an edit keeps;
/// one without a default export, a bad name, no description or a taken name is refused. Code mode
/// is on until turned off, and takes only true or false.
#[tokio::test]
async fn skills_with_code_are_saved_and_kept_through_edits() {
    let (app, dir) = app("code");
    let code = "export default async (args) => args.n + 1\n";
    let item = app.save_code_skill("add-one", "Add one to args.n", code).await.unwrap();
    assert_eq!((item.origin.clone(), app.skill_code(&item).as_deref()), (Origin::Created, Some(code)));
    assert!(app.skill_text(&item).unwrap().contains("skills.add_one(args)"));
    for (name, about, code, why) in [
        ("no-export", "d", "const x = 1", "export default"),
        ("！！", "d", code, "Skill 名称"),
        ("quiet", " ", code, "描述"),
        ("add-one", "d", code, "已经有"),
    ] {
        let err = app.save_code_skill(name, about, code).await.unwrap_err().to_string();
        assert!(err.contains(why), "{name}: {err}");
    }
    let text = app.skill_text(&item).unwrap();
    let edited =
        app.edit_skill(&item.id, &json!({ "name": "add-one", "content": text + "\n更多说明。" })).await.unwrap();
    assert_ne!(edited.revision, item.revision);
    assert_eq!(app.skill_code(&edited).as_deref(), Some(code), "the edit keeps the code");

    assert!(app.code_mode());
    assert!(app.save_code_mode(&json!({ "code": "no" })).is_err(), "only true or false");
    assert!(app.code_mode(), "a refused change changes nothing");
    app.save_code_mode(&json!({ "code": false })).unwrap();
    assert!(!app.code_mode());
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

#[test]
fn folder_preview_distinguishes_a_full_page_from_a_truncated_scan() {
    let (app, dir) = app("scan-limits");
    let root = dir.join("skills");
    for n in 0..500 {
        skill_folder(&root.join(format!("s{n:03}")), "skill", "test");
    }
    let scan = |p: &Path| app.scan_skill_folder(p.to_str().unwrap()).unwrap();
    assert_eq!(scan(&root)["warnings"], json!([]));
    skill_folder(&root.join("s500"), "extra", "test");
    let limited = scan(&root);
    assert_eq!(limited["offers"].as_array().unwrap().len(), 500);
    assert!(limited["warnings"][0].as_str().unwrap().contains("500"));
    let deep = dir.join("deep");
    skill_folder(&deep.join("a/b/c/d/e/f/g/h/i"), "deep", "test");
    assert!(scan(&deep)["warnings"][0].as_str().unwrap().contains("8"));
    assert_eq!(scan(&deep.join("a"))["offers"].as_array().unwrap().len(), 1);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn folder_selection_imports_only_picked_skills() {
    let (app, dir) = app("folder-selection");
    let root = dir.join("local skills");
    let alpha = root.join("alpha");
    let beta = root.join(".agents/skills/beta");
    let outside = dir.join("outside");
    skill_folder(&alpha, "alpha", "v1");
    skill_folder(&alpha.join("examples/inner"), "inner", "part of alpha");
    skill_folder(&beta, "beta", "v1");
    skill_folder(&root.join("unselected"), "unselected", "v1");
    skill_folder(&root.join("node_modules/ignored"), "ignored", "v1");
    skill_folder(&root.join(".git/ignored"), "ignored", "v1");
    skill_folder(&outside, "outside", "v1");
    std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();
    std::fs::write(root.join("mcp.json"), r#"{"mcpServers":{"search":{"command":"search-mcp"}}}"#).unwrap();
    let root = root.canonicalize().unwrap();
    let path = root.to_str().unwrap();

    let preview = app.scan_skill_folder(path).unwrap();
    let offered: Vec<_> = preview["offers"].as_array().unwrap().iter().map(|o| o["path"].as_str().unwrap()).collect();
    assert_eq!(offered, [".agents/skills/beta", "alpha", "unselected"]);
    assert!(app.items().unwrap().is_empty(), "preview does not import");
    let single = app.scan_skill_folder(alpha.to_str().unwrap()).unwrap();
    assert_eq!(single["offers"].as_array().unwrap().len(), 1);
    assert_eq!(single["offers"][0]["path"], "");
    assert_eq!(single["offers"][0]["name"], "alpha");

    for invalid in ["../outside", outside.to_str().unwrap(), "link", "alpha/examples/inner"] {
        let error = app.import_skill_selection(path, &["alpha".into(), invalid.into()]).await.unwrap_err();
        assert!(error.to_string().contains("没有 Skill 路径"));
        assert!(app.items().unwrap().is_empty(), "invalid selection must not import alpha");
    }

    let picked = [".agents/skills/beta".into(), "alpha".into(), "alpha".into()];
    let imported = app.import_skill_selection(path, &picked).await.unwrap();
    assert_eq!(imported["warnings"], json!([]));
    assert_eq!(imported["imported"].as_array().unwrap().len(), 2);
    let alpha_id = imported["imported"][1]["item"].as_str().unwrap();
    let alpha_item = app.item(alpha_id).unwrap();
    assert_eq!(alpha_item.origin, Origin::Folder { path: alpha.canonicalize().unwrap().to_str().unwrap().into() });
    assert!(app.store_dir().join(&alpha_item.revision).join("examples/inner/SKILL.md").is_file());
    assert_eq!(app.import_skill_selection(path, &picked).await.unwrap(), imported);
    let single_import = app.import_skill_selection(alpha.to_str().unwrap(), &[String::new()]).await.unwrap();
    assert_eq!(single_import["imported"][0]["item"], alpha_id);
    assert_eq!(app.items().unwrap().len(), 2);
    let preview = app.scan_skill_folder(path).unwrap();
    assert_eq!(preview["imported"], imported["imported"]);

    skill_folder(&alpha, "alpha", "v2");
    assert_ne!(app.refresh_item(alpha_id).await.unwrap().revision, alpha_item.revision);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn folder_selection_keeps_successes_when_one_copy_fails() {
    let (app, dir) = app("folder-retry");
    let root = dir.join("skills");
    let bad = root.join("bad");
    skill_folder(&root.join("good"), "good", "v1");
    skill_folder(&bad, "bad", "v1");
    let large = bad.join("too-large");
    std::fs::File::create(&large).unwrap().set_len(21 * 1024 * 1024).unwrap();
    let picked = ["good".into(), "bad".into()];

    let result = app.import_skill_selection(root.to_str().unwrap(), &picked).await.unwrap();
    assert_eq!(result["imported"].as_array().unwrap().len(), 1);
    assert_eq!(result["imported"][0]["path"], "good");
    assert!(result["warnings"][0].as_str().unwrap().contains("超过 20MB"));
    std::fs::remove_file(large).unwrap();

    let retry = app.import_skill_selection(root.to_str().unwrap(), &picked).await.unwrap();
    assert_eq!(retry["warnings"], json!([]));
    assert_eq!(retry["imported"][0], result["imported"][0]);
    assert_eq!(retry["imported"][1]["path"], "bad");
    assert_eq!(app.items().unwrap().len(), 2);
    std::fs::remove_dir_all(&dir).unwrap();
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
                env_keys: vec!["GITHUB_TOKEN".into(), "UNSET".into()],
            }
        }
    );
    assert_eq!(app.mcp_values(&item.id).env["GITHUB_TOKEN"], "ghp-secret");
    let view = app.library_view().unwrap().to_string();
    assert!(!view.contains("ghp-secret") && view.contains("\"set\":true"), "{view}");
    let db = std::fs::read(app.cfg.layout.db()).unwrap();
    assert!(!String::from_utf8_lossy(&db).contains("ghp-secret"));
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(app.values_path(&item.id).unwrap()).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);

    // The editor reads the values back, the declared ones only.
    assert_eq!(app.mcp_values_view(&item.id).unwrap(), json!({ "values": { "GITHUB_TOKEN": "ghp-secret" } }));

    // No value keeps the saved one; an empty value clears it; a key no longer listed loses its value.
    let keep = json!({ "name": "github", "transport": "stdio", "command": "npx -y gh-mcp", "vars": [{ "name": "GITHUB_TOKEN" }] });
    app.save_mcp(Some(&item.id), &keep).await.unwrap();
    assert_eq!(app.mcp_values(&item.id).env.get("GITHUB_TOKEN").map(String::as_str), Some("ghp-secret"));
    let clear = json!({ "name": "github", "transport": "stdio", "command": "npx -y gh-mcp", "vars": [{ "name": "GITHUB_TOKEN", "value": "" }] });
    app.save_mcp(Some(&item.id), &clear).await.unwrap();
    assert!(!app.mcp_values(&item.id).env.contains_key("GITHUB_TOKEN"), "an emptied value is gone");
    app.save_mcp(Some(&item.id), &body).await.unwrap();
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

/// Deleting an item takes its saved values along.
#[tokio::test]
async fn deleting_an_item_takes_its_values() {
    let (app, dir) = app("delete");
    let item = app
        .save_mcp(None, &json!({ "name": "fs", "command": "mcp-fs", "vars": [{ "name": "K", "value": "v" }] }))
        .await
        .unwrap();
    assert!(app.values_path(&item.id).unwrap().exists());
    app.delete_item(&item.id).await.unwrap();
    assert!(app.item(&item.id).is_err());
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
    skill_folder(&upstream.join("deep/a/b/c/d/e/f/g/h/i"), "too-deep", "v1");
    skill_folder(&upstream.join("skills/pdf/examples/inner"), "inner", "x"); // part of pdf
    std::fs::create_dir_all(upstream.join("node_modules/x")).unwrap();
    std::fs::write(upstream.join("node_modules/x/SKILL.md"), "no").unwrap();
    std::os::unix::fs::symlink(upstream.join("skills"), upstream.join("linked")).unwrap();
    // MCP servers are added by hand or pasted, never taken from a repository.
    std::fs::write(upstream.join("mcp.json"), r#"{"mcpServers": {"search": {"command": "search-mcp"}}}"#).unwrap();
    sh(&upstream, &["init", "-q"]);
    sh(&upstream, &["add", "-A"]);
    sh(&upstream, &["commit", "-q", "-m", "init"]);

    let url = format!("file://{}", upstream.display());
    let at = || sources::RepoAt { repo: "acme/kit".into(), git_ref: String::new(), path: String::new() };
    let deep = sources::RepoAt { path: "deep".into(), ..at() };
    assert!(app.add_source_at(deep, &url).await.unwrap_err().to_string().contains("只扫描了 8 层"));
    let src = app.add_source_at(at(), &url).await.unwrap();
    let id = src["id"].as_str().unwrap().to_string();
    let offered: Vec<&str> = src["offers"].as_array().unwrap().iter().map(|o| o["path"].as_str().unwrap()).collect();
    assert_eq!(
        offered,
        [".claude/skills/review", "skills/pdf"],
        "no links, node_modules, nested folders or MCP manifests"
    );
    assert!(app.add_source_at(at(), &url).await.is_err(), "the same source twice");
    assert!(app.items().unwrap().is_empty(), "nothing is imported until picked");

    assert!(app.import_from_source(&id, &["mcp.json#search".into()]).await.is_err());
    let added = app.import_from_source(&id, &["skills/pdf".into(), ".claude/skills/review".into()]).await.unwrap();
    let (pdf, review) = (&added[0], &added[1]);
    assert_eq!(pdf.origin, Origin::Git { source: id.clone(), path: "skills/pdf".into() });
    assert!(app.delete_source(&id).await.unwrap_err().to_string().contains("2 项"));

    // Upstream changes pdf, drops review and adds a skill.
    skill_folder(&upstream.join("skills/pdf"), "pdf", "v2");
    std::fs::remove_dir_all(upstream.join(".claude")).unwrap();
    skill_folder(&upstream.join("skills/lint"), "lint", "v1");
    sh(&upstream, &["add", "-A"]);
    sh(&upstream, &["commit", "-q", "-m", "next"]);
    let checked = app.check_sources().await.unwrap();
    assert_eq!(checked["sources"][0]["updateAvailable"], true, "{checked}");

    let done = app.update_source(&id).await.unwrap();
    assert_eq!(done["updated"], json!(["pdf"]));
    assert_eq!(done["missing"], json!(["review"]));
    assert_eq!(done["source"]["updateAvailable"], false);
    assert_ne!(app.item(&pdf.id).unwrap().revision, pdf.revision);
    let view = app.library_view().unwrap();
    let gone = view["items"].as_array().unwrap().iter().find(|i| i["id"] == review.id.as_str()).unwrap();
    assert_eq!(gone["source"]["inSource"], false);

    app.delete_item(&pdf.id).await.unwrap();
    app.delete_item(&review.id).await.unwrap();
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
    assert!(v.get("reach").is_none(), "every item is for every digital worker");
    assert_eq!(v["origin"], json!({ "type": "git", "source": "s1", "path": "mcp.json#docs" }));
    assert_eq!(v["server"]["transport"], "http");
    assert_eq!(serde_json::from_value::<Item>(v).unwrap(), item);
    let skill: Item = serde_json::from_value(json!({
        "id": "i2", "name": "pdf", "origin": { "type": "created" }, "revision": "h", "updatedAt": 0, "kind": "skill"
    }))
    .unwrap();
    assert_eq!(skill.kind(), ItemKind::Skill);
}
