//! Data upgrade tests: old data reaches the latest shape, and a shape change cannot ship without
//! a migration.

use super::*;
use serde_json::json;

/// Where the database of a fresh home for `name` is (`data/janus.db`).
fn temp_path(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("janus-migrate-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let db = Layout::new(&dir).db();
    std::fs::create_dir_all(db.parent().unwrap()).unwrap();
    db
}

/// The home a test database belongs to.
fn home_of(db: &Path) -> &Path {
    db.parent().and_then(Path::parent).unwrap()
}

/// Open the database the way the service does, upgrading it.
fn open(db: &Path) -> Result<(crate::db::Db, Report)> {
    crate::db::Db::open(&Layout::new(home_of(db)))
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
        for step in m.steps {
            if let Step::UndoneBy(later) = step {
                assert!(*later > m.version && MIGRATIONS.iter().any(|n| n.version == *later), "{}", m.name);
                assert_eq!(undone_by(m.version), Some(*later));
            }
        }
    }
}

#[test]
fn a_new_database_starts_at_the_latest_version_without_a_backup() {
    let path = temp_path("fresh");
    let (db, report) = open(&path).unwrap();
    assert_eq!((report.from, report.to, db.data_version().unwrap()), (0, latest(), latest()));
    assert!(report.backup.is_none(), "nothing to back up");
    assert!(!home_of(&path).join("backups").exists());
}

/// What 2026.11.18 wrote reaches the latest shape: the role label is gone from roles and run
/// members, default roles know their origin, the custom command agent is gone, everything else
/// is as it was.
#[test]
fn old_data_upgrades_with_a_backup_and_ends_in_the_latest_shape() {
    let path = old_database("old");
    let before = Connection::open(&path).unwrap();
    let workspace_before: String =
        before.query_row("SELECT data FROM configs WHERE kind = 'workspace'", [], |r| r.get(0)).unwrap();
    // A past run whose members ran on a custom command agent.
    before
        .execute("UPDATE runs SET config = replace(config, '\"kind\": \"claude\"', '\"kind\": \"custom\"') WHERE id = 'run-old'", [])
        .unwrap();
    drop(before);

    let (_db, report) = open(&path).unwrap();
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
    // Version 9: the custom command agent is gone, and the role bound to it is unbound.
    assert_eq!(custom["connection"], "", "the role is left without an agent");
    let agents: Vec<String> = conn
        .prepare("SELECT id FROM configs WHERE kind = 'connection'")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(agents, ["claude"], "only the Claude Code agent is left");
    let workspace_after: String =
        conn.query_row("SELECT data FROM configs WHERE kind = 'workspace'", [], |r| r.get(0)).unwrap();
    // Version 17 renames the default roles it names; nothing else in it changes.
    let mut expected: Value = serde_json::from_str(&workspace_before).unwrap();
    expected["lead"] = json!("pmo");
    for (m, now) in expected["members"].as_array_mut().unwrap().iter_mut().zip(["pmo", "developer", "qa"]) {
        m["role"] = json!(now);
    }
    assert_eq!(
        serde_json::from_str::<Value>(&workspace_after).unwrap(),
        expected,
        "the rest of the workspace is left alone"
    );
    let team: Value = serde_json::from_str(
        &conn.query_row("SELECT config FROM runs WHERE id = 'run-old'", [], |r| r.get::<_, String>(0)).unwrap(),
    )
    .unwrap();
    for m in team["team"].as_array().unwrap() {
        assert!(m.get("role").is_none() && m.get("model_label").is_none() && m["role_id"].is_string(), "{m}");
        assert_eq!(m["runtime"], "claude", "a member that ran on a custom command reads as Claude Code: {m}");
    }
    let members: Vec<crate::orchestrator::Member> =
        serde_json::from_value(team["team"].clone()).unwrap_or_else(|e| panic!("the past run's team reads: {e}"));
    assert!(!members.is_empty());
    for id in ["pmo", "marketing", "product-manager", "developer", "qa", "generalist", "friend"] {
        assert_eq!(config(&conn, "role", id)["seed"]["id"], id, "{id} knows its origin");
    }
    drop(conn);

    // Opening again changes nothing and takes no second backup; a table a migration dropped
    // (the run leases) does not come back with the baseline.
    let (_db, again) = open(&path).unwrap();
    assert!(again.applied.is_empty() && again.backup.is_none());
    assert_eq!(std::fs::read_dir(backup.parent().unwrap()).unwrap().count(), 1);
    let leases: i64 = Connection::open(&path)
        .unwrap()
        .query_row("SELECT count(*) FROM sqlite_master WHERE name = 'leases'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(leases, 0);
}

/// Version 5 said who may use each library item and version 11 takes it back out: every item is
/// for every digital worker, in the library and in the runs, and roles no longer pick items.
/// Version 6: data seeded before the general digital worker existed gets it, untouched by the
/// person. Version 8: a command server says how the MCP router runs it, once per run by default.
#[test]
fn every_item_is_for_everyone_and_the_general_worker_arrives() {
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

    open(&path).unwrap();
    let conn = Connection::open(&path).unwrap();
    for id in ["i-skill", "i-mcp"] {
        assert!(config(&conn, "library_item", id).get("reach").is_none(), "{id}");
    }
    let team: Value = serde_json::from_str(
        &conn.query_row("SELECT config FROM runs WHERE id = 'run-old'", [], |r| r.get::<_, String>(0)).unwrap(),
    )
    .unwrap();
    assert!(team["team"][0]["resources"][0].get("reach").is_none());
    // Version 8: command servers run once per run unless marked otherwise.
    assert_eq!(config(&conn, "library_item", "i-mcp")["server"]["process"], "perRun");
    assert_eq!(team["team"][0]["resources"][0]["server"]["process"], "perRun");
    let general = config(&conn, "role", "generalist");
    assert_eq!((general["name"].as_str(), general["connection"].as_str()), (Some("通用数字员工"), Some("")));
    assert!(general.get("resources").is_none(), "roles no longer pick items");
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

    open(&path).unwrap();
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
    assert_eq!(env.get("MINE"), Some(&Value::Null));
    let secrets = std::fs::read(Layout::new(home_of(&path)).agent_secrets("glm")).unwrap();
    assert_eq!(serde_json::from_slice::<Value>(&secrets).unwrap()["env"]["MINE"], "1");
    assert_eq!(config(&conn, "connection", "ds")["env"]["ANTHROPIC_BASE_URL"], "https://api.deepseek.com/anthropic");
    assert_eq!(config(&conn, "connection", "cx")["status"], "ok", "other agents are left alone");
    assert_eq!(problems(&conn).unwrap(), Vec::<String>::new());
}

/// Version 13: Claude's custom API keeps its id; the key saved apart from the environment becomes
/// its `ANTHROPIC_API_KEY`, and a private value typed into the environment leaves SQLite. Running
/// the step again changes nothing.
#[test]
fn claude_custom_api_keys_become_private_environment() {
    let path = old_database("claude-env");
    let conn = Connection::open(&path).unwrap();
    let c = json!({ "id": "cc", "kind": "claude", "base": "claude", "name": "cc", "auth": "apikey", "bin": "", "prompt": "",
        "env": { "ANTHROPIC_MODEL": "m1", "GATEWAY_TOKEN": "typed-in" }, "status": "ok", "createdAt": 1 });
    conn.execute(
        "INSERT INTO configs (kind, id, data, updated_at) VALUES ('connection', 'cc', ?1, 0)",
        params![c.to_string()],
    )
    .unwrap();
    drop(conn);
    // Where version 13 found it; version 14 moves it.
    let agent = home_of(&path).join("agents/cc");
    std::fs::create_dir_all(&agent).unwrap();
    std::fs::write(agent.join("secrets.json"), r#"{"apiKey":"sk-ant-old"}"#).unwrap();

    open(&path).unwrap();
    let mut conn = Connection::open(&path).unwrap();
    let cc = config(&conn, "connection", "cc");
    assert_eq!(cc["auth"], "apikey", "the auth keeps its id");
    assert_eq!(cc["env"], json!({ "ANTHROPIC_MODEL": "m1", "ANTHROPIC_API_KEY": null, "GATEWAY_TOKEN": null }));
    assert!(!cc.to_string().contains("typed-in") && !cc.to_string().contains("sk-ant-old"));
    let secrets: Value =
        serde_json::from_slice(&std::fs::read(Layout::new(home_of(&path)).agent_secrets("cc")).unwrap()).unwrap();
    assert_eq!(
        (secrets["apiKey"].as_str(), secrets["env"]["GATEWAY_TOKEN"].as_str()),
        (Some("sk-ant-old"), Some("typed-in"))
    );
    let tx = conn.transaction().unwrap();
    let step = Step::Home(steps::agents::migrate_claude_env);
    assert_eq!(apply(&tx, &step, Some(home_of(&path))).unwrap(), 0, "a second run changes nothing");
}

/// Version 15: data seeded before 老友 existed gets it as a default role with no agent bound. A
/// person's own role under that id keeps its own settings; it becomes the default role only when
/// its text is the very text shipped, and otherwise stays theirs as it is.
#[test]
fn old_friend_joins_the_default_roles() {
    let path = old_database("old-friend");
    open(&path).unwrap();
    let conn = Connection::open(&path).unwrap();
    // Added under the id of its time, moved to its current one by version 17.
    let shipped = config(&conn, "role", "friend");
    assert_eq!((shipped["name"].as_str(), shipped["connection"].as_str()), (Some("老友"), Some("")));
    assert_eq!(shipped["seed"]["id"], "friend");
    assert_eq!(problems(&conn).unwrap(), Vec::<String>::new());
    drop(conn);

    let mine = |name: &str, prompt: &str, id: &str| {
        let path = old_database(name);
        let conn = Connection::open(&path).unwrap();
        let role = json!({ "id": "inner-grill", "name": "老友", "desc": shipped["desc"], "prompt": prompt,
            "connection": "", "model": "opus", "effort": "max", "session": {}, "lead": false, "color": "#be185d", "init": "老友" });
        conn.execute(
            "INSERT INTO configs (kind, id, data, updated_at) VALUES ('role', 'inner-grill', ?1, 0)",
            params![role.to_string()],
        )
        .unwrap();
        drop(conn);
        open(&path).unwrap();
        config(&Connection::open(&path).unwrap(), "role", id)
    };
    let copied = mine("old-friend-copied", shipped["prompt"].as_str().unwrap(), "friend");
    assert_eq!(copied["seed"], shipped["seed"], "the shipped text copied by hand becomes the default role");
    assert_eq!(copied["model"], "opus", "keeping the person's own settings");
    let edited = mine("old-friend-edited", "我自己的老友", "inner-grill");
    assert!(edited.get("seed").is_none() && edited["prompt"] == "我自己的老友", "an edited one stays the person's");
}

/// Version 17: the default roles take ids that name what they do, everywhere they are named: the
/// record, a workspace's lead and members, the members of past runs, and the picture's folder.
/// A role of the person's own under an old id, or a default whose new id is taken, stays.
#[test]
fn default_roles_take_ids_that_name_what_they_do() {
    let path = old_database("rename");
    let home = home_of(&path).to_path_buf();
    let conn = Connection::open(&path).unwrap();
    // The person made their own "pmo" before, so the PMO keeps its old id.
    let pmo = json!({ "id": "pmo", "name": "我的 PMO", "desc": "", "prompt": "", "connection": "", "model": "", "effort": "",
        "session": {}, "lead": false, "color": "#64748b", "init": "PM" });
    conn.execute(
        "INSERT INTO configs (kind, id, data, updated_at) VALUES ('role', 'pmo', ?1, 0)",
        params![pmo.to_string()],
    )
    .unwrap();
    drop(conn);
    // A picture where version 16 kept it.
    let picture = home.join("data/roles/implementer/avatar.png");
    std::fs::create_dir_all(picture.parent().unwrap()).unwrap();
    std::fs::write(&picture, b"png").unwrap();

    open(&path).unwrap();
    let conn = Connection::open(&path).unwrap();
    for (then, now) in [("researcher", "marketing"), ("implementer", "developer"), ("reviewer", "qa")] {
        assert!(
            conn.query_row("SELECT 1 FROM configs WHERE kind = 'role' AND id = ?1", [then], |_| Ok(())).is_err(),
            "{then} is gone"
        );
        let role = config(&conn, "role", now);
        assert_eq!((role["id"].as_str(), role["seed"]["id"].as_str()), (Some(now), Some(now)));
    }
    assert_eq!(config(&conn, "role", "coordinator")["seed"]["id"], "coordinator", "its new id was taken");
    assert_eq!(config(&conn, "role", "pmo")["name"], "我的 PMO", "the person's own role is theirs");
    let ws: String = conn.query_row("SELECT data FROM configs WHERE kind = 'workspace'", [], |r| r.get(0)).unwrap();
    let ws: Value = serde_json::from_str(&ws).unwrap();
    assert_eq!(ws["lead"], "coordinator");
    let members: Vec<&str> = ws["members"].as_array().unwrap().iter().map(|m| m["role"].as_str().unwrap()).collect();
    assert_eq!(&members[..3], ["coordinator", "developer", "qa"]);
    let team: String = conn.query_row("SELECT config FROM runs WHERE id = 'run-old'", [], |r| r.get(0)).unwrap();
    let team: Value = serde_json::from_str(&team).unwrap();
    let roles: Vec<&str> = team["team"].as_array().unwrap().iter().map(|m| m["role_id"].as_str().unwrap()).collect();
    assert_eq!(roles, ["coordinator", "developer"], "past runs show the roles they came from");
    assert!(!picture.exists() && home.join("data/roles/developer/avatar.png").exists(), "the picture follows its role");
    assert_eq!(problems(&conn).unwrap(), Vec::<String>::new());
}

#[test]
fn a_database_from_a_newer_janus_is_refused() {
    let path = temp_path("newer");
    open(&path).unwrap();
    Connection::open(&path).unwrap().pragma_update(None, "user_version", latest() + 1).unwrap();
    let err = open(&path).err().expect("refused").to_string();
    assert!(err.contains("更新的 Janus") && err.contains("backups"), "{err}");
}

#[test]
fn the_preview_leaves_the_database_alone() {
    let path = old_database("dry-run");
    let text = dry_run(&path).unwrap();
    assert!(text.contains(&format!("数据版本 0 → {}", latest())) && text.contains("所有数据都符合"), "{text}");
    let conn = Connection::open(&path).unwrap();
    assert_eq!(conn.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0)).unwrap(), 0);
    assert!(!home_of(&path).join("backups").exists());
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
        Step::Add { at: Target::Config(ConfigKind::Workspace), field: "limit", default: || json!(4) },
        Step::Add { at: Target::Config(ConfigKind::Workspace), field: "keep", default: || json!(false) },
        Step::Drop { at: Target::Config(ConfigKind::Workspace), field: "old" },
        Step::Drop { at: Target::Config(ConfigKind::Workspace), field: "gone" },
        Step::Sql("CREATE TABLE extra (id TEXT)"),
    ];
    let changed: Vec<usize> = steps.iter().map(|s| apply(&tx, s, None).unwrap()).collect();
    assert_eq!(changed, [1, 0, 1, 0, 0], "an existing field keeps its value; a missing one is not dropped");
    let again: Vec<usize> = steps[..4].iter().map(|s| apply(&tx, s, None).unwrap()).collect();
    assert_eq!(again, [0, 0, 0, 0]);
    tx.commit().unwrap();
    let data: String = conn.query_row("SELECT data FROM configs WHERE id = 'w'", [], |r| r.get(0)).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&data).unwrap(), json!({ "id": "w", "keep": true, "limit": 4 }));
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

/// Runs in a worktree from before data version 10 record the branch accepting merges back into:
/// the one checked out now when it has the run's start in its history, otherwise none.
#[test]
fn worktree_runs_record_where_accepting_merges_back() {
    let path = old_database("source-branch");
    let repo = home_of(&path).join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(args)
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["commit", "-q", "--allow-empty", "-m", "one"]);
    let base = git(&["rev-parse", "HEAD"]);
    git(&["checkout", "-q", "--orphan", "other"]);
    git(&["commit", "-q", "--allow-empty", "-m", "unrelated"]);
    let other = git(&["rev-parse", "HEAD"]);
    git(&["checkout", "-q", "main"]);
    let conn = Connection::open(&path).unwrap();
    let run = |id: &str, base: &str, worktree: bool| {
        let mut c = json!({ "team": [], "git": true, "baseRef": base });
        if worktree {
            c["worktree"] = json!({ "repo": repo });
        }
        conn.execute(
            "INSERT INTO runs (id, goal, cwd, config, status, created_at) VALUES (?1, 'g', '/x', ?2, 'delivered', 0)",
            params![id, c.to_string()],
        )
        .unwrap();
    };
    run("on-main", &base, true);
    run("elsewhere", &other, true);
    run("in-place", &base, false);
    drop(conn);

    open(&path).unwrap();
    let conn = Connection::open(&path).unwrap();
    let worktree = |id: &str| {
        let c: String = conn.query_row("SELECT config FROM runs WHERE id = ?1", [id], |r| r.get(0)).unwrap();
        serde_json::from_str::<Value>(&c).unwrap()["worktree"].clone()
    };
    assert_eq!(worktree("on-main")["branch"], json!("main"));
    assert_eq!(worktree("elsewhere"), json!({ "repo": repo, "branch": null }), "main lacks its start");
    assert_eq!(worktree("in-place"), Value::Null, "a run without a worktree is left alone");
}

/// Runs no longer have a time or cost ceiling: one that waited for more budget is interrupted (the
/// person continues it), and no snapshot keeps the ceiling it was created with.
#[test]
fn runs_lose_their_budget_ceilings() {
    let path = old_database("no-budgets");
    let conn = Connection::open(&path).unwrap();
    let run = |id: &str, status: &str, config: Value| {
        conn.execute(
            "INSERT INTO runs (id, goal, cwd, config, status, created_at) VALUES (?1, 'g', '/x', ?2, ?3, 0)",
            params![id, config.to_string(), status],
        )
        .unwrap();
    };
    run("waiting", "waiting_input", json!({ "team": [], "costLimitUsd": 2.5, "tokenLimit": 1000 }));
    conn.execute(
        "INSERT INTO events (run_id, seq, ts, type, actor, text) VALUES ('waiting', 1, 0, 'run.budget_exhausted', 'janus', '预算用完')",
        [],
    )
    .unwrap();
    run("done", "delivered", json!({ "team": [] }));
    drop(conn);

    open(&path).unwrap();
    let conn = Connection::open(&path).unwrap();
    let row = |id: &str| {
        let (status, config): (String, String) = conn
            .query_row("SELECT status, config FROM runs WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap();
        (status, serde_json::from_str::<Value>(&config).unwrap())
    };
    let (status, config) = row("waiting");
    assert_eq!(status, "interrupted");
    // Its log says so too, so the run's own page shows it interrupted, with 「从中断处继续」.
    let (seq, kind): (i64, String) = conn
        .query_row("SELECT seq, type FROM events WHERE run_id = 'waiting' ORDER BY seq DESC LIMIT 1", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!((seq, kind.as_str()), (2, "run.interrupted"));
    assert_eq!(config, json!({ "team": [], "tokenLimit": 1000, "lang": "zh" }), "only the cost ceiling goes");
    assert_eq!(row("done"), ("delivered".to_string(), json!({ "team": [], "lang": "zh" })));
}

/// Runs made before Loops were the person's own; a Loop's dispatch key is unique in its workspace.
#[test]
fn runs_record_who_they_are_for() {
    let path = old_database("origin");
    let conn = Connection::open(&path).unwrap();
    conn.execute(
        "INSERT INTO runs (id, goal, cwd, config, status, created_at) VALUES ('old', 'g', '/x', '{\"workspace\":{\"id\":\"w\"}}', 'delivered', 0)",
        [],
    )
    .unwrap();
    drop(conn);
    open(&path).unwrap();
    let conn = Connection::open(&path).unwrap();
    let (origin, loop_id): (String, Option<String>) = conn
        .query_row("SELECT origin, loop_id FROM runs WHERE id = 'old'", [], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();
    assert_eq!((origin.as_str(), loop_id), ("user", None));
    let dispatch = |id: &str, ws: &str| {
        conn.execute(
            "INSERT INTO runs (id, goal, cwd, config, status, created_at, origin, loop_id, dispatch_key)
             VALUES (?1, 'g', '/x', ?2, 'queued', 0, 'loop', 'l1', 'k1')",
            params![id, json!({ "workspace": { "id": ws } }).to_string()],
        )
    };
    dispatch("a", "w").unwrap();
    assert!(dispatch("b", "w").is_err(), "one run per key in a workspace");
    dispatch("c", "other").unwrap();
}

/// A Loop's rhythm becomes a schedule: an interval comes round on the clock, a self-paced Loop
/// every 30 minutes, a manual one never by itself.
#[test]
fn loop_rhythms_become_schedules() {
    let path = old_database("schedule");
    let conn = Connection::open(&path).unwrap();
    let rhythm = |id: &str, rhythm: Value| {
        let data = json!({ "id": id, "workspace": "w", "name": "", "prompt": "p", "rhythm": rhythm, "state": "active", "run": "r" });
        conn.execute(
            "INSERT INTO configs (kind, id, data, updated_at) VALUES ('loop', ?1, ?2, 0)",
            params![id, data.to_string()],
        )
        .unwrap();
    };
    rhythm("half", json!({ "kind": "interval", "minutes": 30 }));
    rhythm("hours", json!({ "kind": "interval", "minutes": 120 }));
    rhythm("day", json!({ "kind": "interval", "minutes": 1440 }));
    rhythm("self", json!({ "kind": "self_paced" }));
    rhythm("manual", json!({ "kind": "manual" }));
    drop(conn);
    open(&path).unwrap();
    let conn = Connection::open(&path).unwrap();
    let tz = crate::schedule::local_zone();
    for (id, cron) in
        [("half", "*/30 * * * *"), ("hours", "0 */2 * * *"), ("day", "0 0 * * *"), ("self", "*/30 * * * *")]
    {
        let def = config(&conn, "loop", id);
        assert_eq!(def["schedule"], json!({ "cron": cron, "tz": tz }), "{id}");
        assert!(def.get("rhythm").is_none());
        assert!(
            crate::data_shape::check(ConfigKind::Loop, &def).is_empty(),
            "{id}: {:?}",
            crate::data_shape::check(ConfigKind::Loop, &def)
        );
    }
    assert_eq!(config(&conn, "loop", "manual")["schedule"], Value::Null);
}

/// The paths shared one by one before mean nothing once agents' HOME mirrors the home folder: they
/// go, and the hidden list starts as the defaults.
#[test]
fn shared_host_paths_go() {
    let path = old_database("host-links");
    let conn = Connection::open(&path).unwrap();
    for p in [".config/gh", "Library/Keychains"] {
        conn.execute(
            "INSERT INTO configs (kind, id, data, updated_at) VALUES ('host_link', ?1, ?2, 0)",
            params![p, json!({ "path": p }).to_string()],
        )
        .unwrap();
    }
    drop(conn);
    open(&path).unwrap();
    let conn = Connection::open(&path).unwrap();
    let left: i64 = conn
        .query_row("SELECT COUNT(*) FROM configs WHERE kind IN ('host_link', 'host_env')", [], |r| r.get(0))
        .unwrap();
    assert_eq!(left, 0);
}

/// Digital employees are listed by group: the default ones join the group they ship in, the
/// person's own start in none.
#[test]
fn default_roles_join_their_groups() {
    let path = old_database("groups");
    open(&path).unwrap();
    let conn = Connection::open(&path).unwrap();
    for (id, group) in [("pmo", "产品上线"), ("qa", "产品上线"), ("generalist", "通用"), ("rmwm60uas", "")] {
        assert_eq!(config(&conn, "role", id)["group"], group, "{id}");
    }
}
