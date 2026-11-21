//! The layout: what it recognizes, and version 14 moving a home of the first layout into it.

use super::*;
use rusqlite::{Connection, params};
use serde_json::{Value, json};

fn temp_home(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("janus-layout-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // Recorded paths are resolved ones (`/private/var/…` for `/var/…`), as a worktree's is.
    dir.canonicalize().unwrap()
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Only artifacts are served and only stored attachments are sent; a path that leads out does
/// not count, and anything at the top of the home outside `TOP` is someone else's.
#[test]
fn it_recognizes_only_what_janus_stored() {
    let home = temp_home("recognize");
    let layout = Layout::new(&home);
    let run = layout.run(RunKind::Goal, "r1");
    let report = run.artifacts("T1").join("T1-a1-report.md");
    let image = layout.run(RunKind::Chat, "c1").attachments().join("1.png");
    write(&report, "done");
    write(&image, "png");
    write(&layout.db(), "");
    assert!(layout.is_artifact(&report));
    assert!(!layout.is_artifact(&layout.db()), "the database is not an artifact");
    assert!(!layout.is_artifact(&run.artifacts("T1").join("../../../../janus.db")));
    assert!(layout.is_attachment(&image) && !layout.is_attachment(&report));
    write(&home.join("signing/cert.pem"), "");
    write(&home.join(".DS_Store"), "");
    assert_eq!(layout.foreign(), ["signing"]);
    let _ = std::fs::remove_dir_all(&home);
}

/// A home of the first layout, with a run in a worktree, a chat, an agent and a library item,
/// moves into the categories: every file lands in its new place, every recorded path follows
/// it (the database, the agent's own files, Claude Code's project names, git's worktree links),
/// and running the step again changes nothing.
#[test]
fn version_14_moves_the_first_layout_into_categories() {
    let h = temp_home("v14");
    let hs = h.to_string_lossy().to_string();
    let p = |rel: &str| h.join(rel);

    // The repository and the run's worktree, made by git where the first layout kept it.
    let repo = p("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "one"]);
    git(&repo, &["worktree", "add", "-q", "--detach", &format!("{hs}/worktrees/rg"), "HEAD"]);

    // The database, at the top, at data version 13.
    let mut conn = Connection::open(p("janus.db")).unwrap();
    crate::migrations::upgrade(&mut conn, None).unwrap();
    conn.pragma_update(None, "user_version", 13).unwrap();
    let report = format!("{hs}/artifacts/rg/T1/T1-a1-report.md");
    let goal = json!({ "team": [{ "id": "lead", "runtime": "cc" }], "worktree": { "repo": repo },
        "goalImages": [{ "path": format!("{hs}/runs/rg/attachments/1.png") }] });
    conn.execute(
        "INSERT INTO runs (id, kind, goal, cwd, config, status, created_at) VALUES ('rg', 'goal', 'g', ?1, ?2, 'delivered', 0)",
        params![format!("{hs}/worktrees/rg"), goal.to_string()],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO runs (id, kind, goal, cwd, config, status, created_at) VALUES ('rc', 'chat', 'c', ?1, '{}', 'running', 0)",
        params![format!("{hs}/chats/rc")],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO artifacts (run_id, task, attempt, name, kind, path, sha256, bytes, created_at) VALUES ('rg', 'T1', 1, 'r', 'report', ?1, '', 0, 0)",
        params![report],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO events (run_id, seq, ts, type, actor, text, data) VALUES ('rg', 1, 0, 'artifact.created', 'janus', ?1, ?2)",
        params![format!("产物：{report}"), json!({ "path": report }).to_string()],
    )
    .unwrap();
    // `…/logs/rg` is not a prefix of `…/logs/rgx`: only whole names move.
    conn.execute(
        "INSERT INTO events (run_id, seq, ts, type, actor, text) VALUES ('rg', 2, 0, 'tool.call', 'lead', ?1)",
        params![format!("{hs}/logs/rgx/a.log")],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO configs (kind, id, data, updated_at) VALUES ('library_item', 'i1', ?1, 0)",
        params![json!({ "id": "i1", "name": "gh" }).to_string()],
    )
    .unwrap();
    drop(conn);

    // The files of the first layout.
    let chat_key = |dir: &str| dir.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect::<String>();
    let old_project = format!("agents/cc/home/projects/{}", chat_key(&format!("{hs}/chats/rc")));
    for (rel, text) in [
        ("artifacts/rg/T1/T1-a1-report.md", "done"),
        ("runs/rg/members/lead/bundle/skills/x/SKILL.md", "x"),
        ("runs/rg/attachments/1.png", "png"),
        ("logs/rg/lead-goal.log", ""),
        ("logs/rc/chat.log", ""),
        ("chats/rc/notes.md", "scratch"),
        ("worktrees/deleted/old.patch", "diff"),
        (&format!("{old_project}/s1.jsonl"), &format!("{{\"cwd\":\"{hs}/chats/rc\"}}")),
        ("agents/cc/home/.claude.json", &format!("{{\"projects\":{{\"{hs}/chats/rc\":{{}}}}}}")),
        ("agents/cc/root/.npmrc", ""),
        // What an agent installed for itself writes its own place down.
        ("agents/cc/root/.cargo/env", &format!("export PATH=\"{hs}/agents/cc/root/.cargo/bin:$PATH\"")),
        ("agents/cc/root/.npm/tool", ""),
        ("agents/cc/secrets.json", "{}"),
        ("agents/cc/tmp/scratch", ""),
        ("agents/cc/probe/.keep", ""),
        ("agents/cc/test.log", "ok"),
        ("library/store/abc/SKILL.md", "s"),
        ("library/sources/s1/repo/README.md", ""),
        ("library/servers/i1/root/.cache/x", ""),
        ("library/servers/i1/tmp/x", ""),
        ("library/tools/i1-rev1.json", "[]"),
        ("library/tools/gone-rev1.json", "[]"),
        ("secrets/library/i1.json", "{}"),
        ("logs/library/mcp-gh.log", ""),
        ("adapters/claude/1.0.0/package.json", "{}"),
        ("prices/models.json", "{}"),
        ("updates/previous/Janus.app/Info.plist", ""),
        ("permissions/review.json", "{}"),
        ("signing/cert.pem", ""),
    ] {
        write(&p(rel), text);
    }

    std::fs::create_dir_all(p("agents/cc/root/.local/bin")).unwrap();
    std::os::unix::fs::symlink(p("agents/cc/root/.npm/tool"), p("agents/cc/root/.local/bin/tool")).unwrap();

    let layout = Layout::new(&h);
    adopt_db(&layout).unwrap();
    let (db, report14) = crate::db::Db::open(&layout).unwrap();
    assert_eq!((report14.from, report14.to), (13, 14));
    assert!(report14.backup.is_some_and(|b| b.starts_with(layout.backups())));

    let (rg, rc) = (layout.run(RunKind::Goal, "rg"), layout.run(RunKind::Chat, "rc"));
    for path in [
        layout.db(),
        rg.artifacts("T1").join("T1-a1-report.md"),
        rg.bundle("lead").join("skills/x/SKILL.md"),
        rg.attachments().join("1.png"),
        rg.log("lead-goal.log"),
        rc.log("chat.log"),
        rc.work.join("notes.md"),
        rg.work.join(".git"),
        layout.deleted_runs().join("old.patch"),
        layout.agent_config("cc").join(".claude.json"),
        layout.agent_home("cc").join(".npmrc"),
        layout.agent_secrets("cc"),
        layout.agent_logs("cc").join("test.log"),
        layout.skills().join("abc/SKILL.md"),
        layout.source("s1").join("repo/README.md"),
        layout.server_home("i1").join(".cache/x"),
        layout.mcp_tools("i1").join("rev1.json"),
        layout.mcp_values("i1"),
        layout.library_log("i1"),
        layout.adapters().join("claude/1.0.0/package.json"),
        layout.prices(),
        layout.updates().join("previous/Janus.app/Info.plist"),
        layout.app_state().join("permissions.json"),
    ] {
        assert!(path.exists(), "{} is there", path.display());
    }
    assert_eq!(layout.foreign(), ["repo", "signing"], "only what is not Janus's is left at the top");
    assert!(!layout.agent_tmp("cc").exists() && !layout.server_tmp("i1").exists(), "scratch is not kept");

    // The recorded paths follow the files.
    let conn = Connection::open(layout.db()).unwrap();
    let one = |sql: &str| conn.query_row(sql, [], |r| r.get::<_, String>(0)).unwrap();
    let new_report = rg.artifacts("T1").join("T1-a1-report.md").to_string_lossy().to_string();
    assert_eq!(one("SELECT path FROM artifacts"), new_report);
    assert_eq!(one("SELECT text FROM events WHERE seq = 1"), format!("产物：{new_report}"));
    assert_eq!(
        serde_json::from_str::<Value>(&one("SELECT data FROM events WHERE seq = 1")).unwrap()["path"],
        json!(new_report)
    );
    assert_eq!(
        one("SELECT text FROM events WHERE seq = 2"),
        format!("{hs}/logs/rgx/a.log"),
        "a longer name is left alone"
    );
    assert_eq!(one("SELECT cwd FROM runs WHERE id = 'rg'"), rg.work.to_string_lossy());
    assert_eq!(one("SELECT cwd FROM runs WHERE id = 'rc'"), rc.work.to_string_lossy());
    let config: Value = serde_json::from_str(&one("SELECT config FROM runs WHERE id = 'rg'")).unwrap();
    assert_eq!(config["goalImages"][0]["path"], json!(rg.attachments().join("1.png")));
    assert_eq!(config["worktree"]["repo"], json!(repo), "what is outside the home stays");

    // The agent's own files: the project is named after the new folder and names it.
    let project = layout.agent_config("cc").join("projects").join(chat_key(&rc.work.to_string_lossy()));
    let transcript = std::fs::read_to_string(project.join("s1.jsonl")).unwrap();
    assert!(transcript.contains(&rc.work.to_string_lossy().to_string()), "{transcript}");
    let settings = std::fs::read_to_string(layout.agent_config("cc").join(".claude.json")).unwrap();
    assert!(!settings.contains(&format!("{hs}/chats/")), "{settings}");
    // And what it installed in its HOME still finds itself there.
    let own = layout.agent_home("cc");
    let env = std::fs::read_to_string(own.join(".cargo/env")).unwrap();
    assert!(env.contains(&own.join(".cargo/bin").to_string_lossy().to_string()), "{env}");
    assert_eq!(std::fs::read_link(own.join(".local/bin/tool")).unwrap(), own.join(".npm/tool"));

    // Git knows where its worktree went, and it still works there.
    let listed = git(&repo, &["worktree", "list", "--porcelain"]);
    assert!(listed.contains(&rg.work.to_string_lossy().to_string()), "{listed}");
    git(&rg.work, &["status", "--short"]);

    // Again: everything is in place already.
    drop(db);
    let mut conn = Connection::open(layout.db()).unwrap();
    let tx = conn.transaction().unwrap();
    assert_eq!(move_to_categories(&tx, Some(&h)).unwrap(), 0, "a second run changes nothing");
    drop(tx);
    let _ = std::fs::remove_dir_all(&h);
}
