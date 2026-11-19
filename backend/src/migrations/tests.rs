//! Data upgrade tests: old data reaches the latest shape, and a shape change cannot ship without
//! a migration.

use super::*;
use serde_json::json;

fn temp_path(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("janus-migrate-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("janus.db")
}

/// A database as Janus 2026.11.18 left it (`testdata/upgrade/v0.sql`, written by that release).
fn old_database(name: &str) -> PathBuf {
    let path = temp_path(name);
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(BASELINE).unwrap();
    conn.execute_batch(include_str!("../../testdata/upgrade/v0.sql")).unwrap();
    path
}

fn config(conn: &Connection, kind: &str, id: &str) -> Value {
    let data: String = conn
        .query_row("SELECT data FROM configs WHERE kind = ?1 AND id = ?2", params![kind, id], |r| r.get(0))
        .unwrap();
    serde_json::from_str(&data).unwrap()
}

#[test]
fn versions_count_up_from_one() {
    for (i, m) in MIGRATIONS.iter().enumerate() {
        assert_eq!(m.version as usize, i + 1, "{}", m.name);
        assert!(!m.name.is_empty() && !m.steps.is_empty());
    }
}

#[test]
fn a_new_database_starts_at_the_latest_version_without_a_backup() {
    let path = temp_path("fresh");
    let (db, report) = crate::db::Db::open(&path).unwrap();
    assert_eq!((report.from, report.to, db.data_version().unwrap()), (0, latest(), latest()));
    assert!(report.backup.is_none(), "nothing to back up");
    assert!(!path.parent().unwrap().join("backups").exists());
}

/// What 2026.11.18 wrote reaches the latest shape: the role label is gone from roles and run
/// members, default roles know their origin, everything else is as it was.
#[test]
fn old_data_upgrades_with_a_backup_and_ends_in_the_latest_shape() {
    let path = old_database("old");
    let before = Connection::open(&path).unwrap();
    let workspace_before: String =
        before.query_row("SELECT data FROM configs WHERE kind = 'workspace'", [], |r| r.get(0)).unwrap();
    drop(before);

    let (_db, report) = crate::db::Db::open(&path).unwrap();
    assert_eq!((report.from, report.to), (0, latest()));
    let backup = report.backup.expect("a backup before upgrading");
    assert!(backup.exists() && backup.file_name().unwrap().to_string_lossy().starts_with("janus-v0-"));
    assert_eq!(report.applied.iter().map(|a| a.0).collect::<Vec<_>>(), (1..=latest()).collect::<Vec<_>>());

    let conn = Connection::open(&path).unwrap();
    assert_eq!(problems(&conn).unwrap(), Vec::<String>::new(), "every object has the latest shape");
    let custom: String = conn
        .query_row("SELECT id FROM configs WHERE kind = 'role' AND data LIKE '%测试员%'", [], |r| r.get(0))
        .unwrap();
    let custom = config(&conn, "role", &custom);
    assert!(custom.get("role").is_none() && custom.get("seed").is_none() && custom["prompt"] == "只写测试");
    let workspace_after: String =
        conn.query_row("SELECT data FROM configs WHERE kind = 'workspace'", [], |r| r.get(0)).unwrap();
    assert_eq!(workspace_after, workspace_before, "objects no migration touches are left alone");
    let team: Value = serde_json::from_str(
        &conn.query_row("SELECT config FROM runs WHERE id = 'run-old'", [], |r| r.get::<_, String>(0)).unwrap(),
    )
    .unwrap();
    for m in team["team"].as_array().unwrap() {
        assert!(m.get("role").is_none() && m.get("model_label").is_none() && m["role_id"].is_string(), "{m}");
    }
    for id in ["coordinator", "researcher", "product-manager", "implementer", "reviewer", "generalist"] {
        assert_eq!(config(&conn, "role", id)["seed"]["id"], id, "{id} knows its origin");
    }
    drop(conn);

    // Opening again changes nothing and takes no second backup; a table a migration dropped
    // (the run leases) does not come back with the baseline.
    let (_db, again) = crate::db::Db::open(&path).unwrap();
    assert!(again.applied.is_empty() && again.backup.is_none());
    assert_eq!(std::fs::read_dir(backup.parent().unwrap()).unwrap().count(), 1);
    let leases: i64 = Connection::open(&path)
        .unwrap()
        .query_row("SELECT count(*) FROM sqlite_master WHERE name = 'leases'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(leases, 0);
}

/// Version 5: each library item, and each item pinned in a run, says who may use it: a skill
/// every digital worker, an MCP server only the roles that pick it. Version 6: data seeded before
/// the general digital worker existed gets it, untouched by the person. Version 8: a command
/// server says how the MCP router runs it, once per run by default.
#[test]
fn items_say_who_uses_them_and_the_general_worker_arrives() {
    let path = old_database("reach");
    let conn = Connection::open(&path).unwrap();
    let skill = json!({ "id": "i-skill", "name": "pdf", "description": "", "origin": { "type": "created" }, "revision": "r1", "updatedAt": 0, "kind": "skill" });
    let mcp = json!({ "id": "i-mcp", "name": "db", "description": "", "origin": { "type": "created" }, "revision": "r2", "updatedAt": 0, "kind": "mcp",
        "server": { "transport": "stdio", "command": "db-mcp", "args": [], "envKeys": [] } });
    for item in [&skill, &mcp] {
        conn.execute(
            "INSERT INTO configs (kind, id, data, updated_at) VALUES ('library_item', ?1, ?2, 0)",
            params![item["id"].as_str().unwrap(), item.to_string()],
        )
        .unwrap();
    }
    let mut snapshot: Value = serde_json::from_str(
        &conn.query_row("SELECT config FROM runs WHERE id = 'run-old'", [], |r| r.get::<_, String>(0)).unwrap(),
    )
    .unwrap();
    snapshot["team"][0]["resources"] = json!([mcp]);
    conn.execute("UPDATE runs SET config = ?1 WHERE id = 'run-old'", params![snapshot.to_string()]).unwrap();
    drop(conn);

    crate::db::Db::open(&path).unwrap();
    let conn = Connection::open(&path).unwrap();
    assert_eq!(config(&conn, "library_item", "i-skill")["reach"], "everyone");
    assert_eq!(config(&conn, "library_item", "i-mcp")["reach"], "picked");
    let team: Value = serde_json::from_str(
        &conn.query_row("SELECT config FROM runs WHERE id = 'run-old'", [], |r| r.get::<_, String>(0)).unwrap(),
    )
    .unwrap();
    assert_eq!(team["team"][0]["resources"][0]["reach"], "picked");
    // Version 8: command servers run once per run unless marked otherwise.
    assert_eq!(config(&conn, "library_item", "i-mcp")["server"]["process"], "perRun");
    assert_eq!(team["team"][0]["resources"][0]["server"]["process"], "perRun");
    let general = config(&conn, "role", "generalist");
    assert_eq!((general["name"].as_str(), general["connection"].as_str()), (Some("通用数字员工"), Some("")));
    assert_eq!(problems(&conn).unwrap(), Vec::<String>::new());
}

/// Version 7: an agent made from a removed preset becomes a Claude Code agent on an API key, with
/// the preset's variables in its own environment, unless the person had set or unset them there.
#[test]
fn preset_agents_become_claude_code_agents_with_their_variables() {
    let path = old_database("presets");
    let conn = Connection::open(&path).unwrap();
    let agent = |id: &str, kind: &str, base: &str, auth: &str, env: Value, extra: Value| {
        let mut c = json!({ "id": id, "kind": kind, "base": base, "name": id, "auth": auth, "bin": "", "prompt": "",
            "env": env, "status": "ok", "createdAt": 1 });
        for (k, v) in extra.as_object().unwrap() {
            c[k] = v.clone();
        }
        conn.execute(
            "INSERT INTO configs (kind, id, data, updated_at) VALUES ('connection', ?1, ?2, 0)",
            params![id, c.to_string()],
        )
        .unwrap();
    };
    agent(
        "glm",
        "glm-claude",
        "claude",
        "",
        json!({ "API_TIMEOUT_MS": "", "ANTHROPIC_DEFAULT_OPUS_MODEL": "glm-6", "MINE": "1" }),
        json!({ "endpoint": "zai" }),
    );
    agent("ds", "deepseek-claude", "claude", "", json!({}), json!({}));
    agent("cx", "codex", "codex", "chatgpt", json!({}), json!({}));
    drop(conn);

    crate::db::Db::open(&path).unwrap();
    let conn = Connection::open(&path).unwrap();
    let glm = config(&conn, "connection", "glm");
    assert_eq!(
        (glm["kind"].as_str(), glm["auth"].as_str(), glm["status"].as_str()),
        (Some("claude"), Some("apikey"), Some("untested"))
    );
    assert!(glm.get("endpoint").is_none());
    let env = &glm["env"];
    assert_eq!(env["ANTHROPIC_BASE_URL"], "https://api.z.ai/api/anthropic", "the endpoint it was given");
    assert_eq!(env["ANTHROPIC_DEFAULT_OPUS_MODEL"], "glm-6", "the person's own value wins");
    assert!(env.get("API_TIMEOUT_MS").is_none(), "a variable the person unset stays unset");
    assert_eq!(env["MINE"], "1");
    assert_eq!(config(&conn, "connection", "ds")["env"]["ANTHROPIC_BASE_URL"], "https://api.deepseek.com/anthropic");
    assert_eq!(config(&conn, "connection", "cx")["status"], "ok", "other agents are left alone");
    assert_eq!(problems(&conn).unwrap(), Vec::<String>::new());
}

#[test]
fn a_database_from_a_newer_janus_is_refused() {
    let path = temp_path("newer");
    crate::db::Db::open(&path).unwrap();
    Connection::open(&path).unwrap().pragma_update(None, "user_version", latest() + 1).unwrap();
    let err = crate::db::Db::open(&path).err().expect("refused").to_string();
    assert!(err.contains("更新的 Janus") && err.contains("backups"), "{err}");
}

#[test]
fn the_preview_leaves_the_database_alone() {
    let path = old_database("dry-run");
    let text = dry_run(&path).unwrap();
    assert!(text.contains(&format!("数据版本 0 → {}", latest())) && text.contains("所有数据都符合"), "{text}");
    let conn = Connection::open(&path).unwrap();
    assert_eq!(conn.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0)).unwrap(), 0);
    assert!(!path.parent().unwrap().join("backups").exists());
}

/// Field steps change only what they apply to, so running one again changes nothing.
#[test]
fn field_steps_apply_once() {
    let mut conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(BASELINE).unwrap();
    conn.execute(
        "INSERT INTO configs (kind, id, data, updated_at) VALUES ('workspace', 'w', ?1, 1)",
        params![json!({ "id": "w", "old": 1, "keep": true }).to_string()],
    )
    .unwrap();
    let tx = conn.transaction().unwrap();
    let steps = [
        Step::Add { at: Target::Config("workspace"), field: "limit", default: || json!(4) },
        Step::Add { at: Target::Config("workspace"), field: "keep", default: || json!(false) },
        Step::Rename { at: Target::Config("workspace"), from: "old", to: "new" },
        Step::Drop { at: Target::Config("workspace"), field: "gone" },
        Step::Sql("CREATE TABLE extra (id TEXT)"),
    ];
    let changed: Vec<usize> = steps.iter().map(|s| apply(&tx, s).unwrap()).collect();
    assert_eq!(changed, [1, 0, 1, 0, 0], "an existing field keeps its value; a missing one is not dropped");
    let again: Vec<usize> = steps[..4].iter().map(|s| apply(&tx, s).unwrap()).collect();
    assert_eq!(again, [0, 0, 0, 0]);
    tx.commit().unwrap();
    let data: String = conn.query_row("SELECT data FROM configs WHERE id = 'w'", [], |r| r.get(0)).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&data).unwrap(), json!({ "id": "w", "new": 1, "keep": true, "limit": 4 }));
}

/// The registered shapes and the tables match `testdata/data_shape.json`. A change to either is a
/// change of stored data: add a migration, then refresh the file with `just data-shape`.
#[test]
fn the_data_shape_matches_its_snapshot() {
    let recorded: Value = serde_json::from_str(include_str!("../../testdata/data_shape.json")).unwrap();
    let now = crate::data_shape::snapshot().unwrap();
    if recorded == now {
        return;
    }
    if recorded["dataVersion"] == now["dataVersion"] {
        panic!(
            "存储的数据结构变了，但没有新的迁移。在 backend/src/migrations.rs 的 MIGRATIONS 末尾加一条迁移，让旧数据变成新的结构，再运行 just data-shape 更新 testdata/data_shape.json。"
        );
    }
    panic!("有新的迁移：运行 just data-shape 更新 testdata/data_shape.json。");
}
