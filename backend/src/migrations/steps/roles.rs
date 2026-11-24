//! The default roles' data versions: where a default came from (2), defaults added after the
//! person's data was seeded (6, 15), and the defaults' new ids (17). Defaults are named as they
//! were at each version and get today's text (`seed_once_named`).

use crate::agents::{RoleSeed, SEEDS, text_hash};
use crate::db::ConfigKind;
use anyhow::Result;
use rusqlite::OptionalExtension;
use serde_json::{Value, json};
use std::path::Path;

/// Every `desc` and `prompt` each default role has shipped with, newest first, by hash. Data
/// version 2 uses it to tell an untouched default role from one the person edited.
const SEED_HISTORY: &[(&str, &str, &[&str])] = &[
    ("coordinator", "desc", &["11c26d05577fd4dd", "c073bc2d506f6974"]),
    (
        "coordinator",
        "prompt",
        &["e65ffc65888ae583", "a81c047b83d8845d", "00dfa19b6610feb6", "0cad66d4ff1444f7", "7dfaede38661aa89"],
    ),
    ("researcher", "desc", &["8f15e8966e580c22", "3c1578d280be92ab"]),
    ("researcher", "prompt", &["07d89d72db780be3", "ecde922ff521160c", "f371c15c3915aeb9"]),
    ("product-manager", "desc", &["55820f7da51d1b35", "6575fe20b36ad7cb"]),
    ("product-manager", "prompt", &["c1e2e315f041c00e", "7c9157e10580d1c8", "0c45315e34958031", "8432df1724ece14d"]),
    ("implementer", "desc", &["3f1d72fd83df989f", "0cff94e51cadbae7"]),
    ("implementer", "prompt", &["e688dd4dbc6bc43d", "a256fd34f7e21459", "e3dabe553ae3b911"]),
    ("reviewer", "desc", &["3a457fb394888f0b", "b91cafdf208c7a65"]),
    ("reviewer", "prompt", &["9cc3fe77d56480b7", "7ffd4c34321ef2e0"]),
];

/// Data version 2: default roles created before roles remembered their origin get a `seed`. A
/// field whose text is one Janus shipped counts as untouched (and moves to the current text on
/// start); an edited one is marked as given the newest earlier text, so the app offers the
/// current one instead of replacing it. Only the default roles of that time count (those in
/// `SEED_HISTORY`): a role of the person's under the id of a later default is theirs.
pub(crate) fn record_seed_origins(tx: &rusqlite::Transaction) -> Result<usize> {
    super::super::edit_configs(tx, ConfigKind::Role, |id, role| {
        if !SEED_HISTORY.iter().any(|h| h.0 == id) {
            return false;
        }
        let Some(r) = seed_once_named(id) else { return false };
        if role.contains_key("seed") {
            return false;
        }
        // Under the id it had then: data version 17 moves it to the current one.
        let mut seed = json!({ "id": id });
        for (field, text) in [("desc", r.desc), ("prompt", r.prompt)] {
            let shipped = text_hash(text);
            let history: Vec<&str> =
                SEED_HISTORY.iter().filter(|h| h.0 == id && h.1 == field).flat_map(|h| h.2.iter().copied()).collect();
            let now = text_hash(role.get(field).and_then(Value::as_str).unwrap_or_default());
            seed[field] = json!(if now == shipped || history.contains(&now.as_str()) {
                now
            } else {
                history.iter().find(|h| **h != shipped).map_or(shipped.clone(), |h| h.to_string())
            });
        }
        role.insert("seed".into(), seed);
        true
    })
}

/// Default roles whose id changed in data version 17, the id they had then first: ids name what
/// each one does (项目管理, 市场运营, 开发, 质量, 老友).
const RENAMED: &[(&str, &str)] = &[
    ("coordinator", "pmo"),
    ("researcher", "marketing"),
    ("implementer", "developer"),
    ("reviewer", "qa"),
    ("inner-grill", "friend"),
];

/// The default role that went by `id` at some data version: older migrations name roles as they
/// were then, and get today's text.
pub(crate) fn seed_once_named(id: &str) -> Option<&'static RoleSeed> {
    let now = RENAMED.iter().find(|(then, _)| *then == id).map_or(id, |(_, now)| *now);
    SEEDS.iter().find(|r| r.id == now)
}

/// Data version 6: the general digital worker, for data seeded before it existed (`add_seed`).
pub(crate) fn add_generalist(tx: &rusqlite::Transaction) -> Result<usize> {
    add_seed(tx, "generalist")
}

/// Data version 15: 老友, for data seeded before it existed (`add_seed`), under the id it had then.
pub(crate) fn add_old_friend(tx: &rusqlite::Transaction) -> Result<usize> {
    add_seed(tx, "inner-grill")
}

/// Data version 17: the default roles move to ids that name what they do (`RENAMED`). A default
/// role (its `seed` says so) takes its new id everywhere it is named: its record, workspaces
/// (members and lead), the members of runs and chats, and its picture's folder. A role of the
/// person's own under an old id, or one whose new id is taken, is left as it is.
// @project-doc docs/architecture/20261031-data_model.md#migrations
pub(crate) fn rename_default_roles(tx: &rusqlite::Transaction, home: Option<&Path>) -> Result<usize> {
    let mut moved = vec![];
    for &(then, now) in RENAMED {
        let row = |id: &str| -> Result<Option<Value>> {
            let data: Option<String> = tx
                .query_row(
                    "SELECT data FROM configs WHERE kind = ?1 AND id = ?2",
                    rusqlite::params![ConfigKind::Role.as_str(), id],
                    |r| r.get(0),
                )
                .optional()?;
            Ok(data.map(|d| serde_json::from_str(&d)).transpose()?)
        };
        let Some(mut role) = row(then)? else { continue };
        if role["seed"]["id"] != then || row(now)?.is_some() {
            continue;
        }
        role["id"] = json!(now);
        role["seed"]["id"] = json!(now);
        tx.execute(
            "DELETE FROM configs WHERE kind = ?1 AND id = ?2",
            rusqlite::params![ConfigKind::Role.as_str(), then],
        )?;
        tx.execute(
            "INSERT INTO configs (kind, id, data, updated_at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![ConfigKind::Role.as_str(), now, role.to_string(), crate::util::now_ms()],
        )?;
        if let Some(home) = home {
            // Where version 16 kept pictures (`Layout::role`).
            let (from, to) = (home.join("data/roles").join(then), home.join("data/roles").join(now));
            if from.exists() && !to.exists() {
                std::fs::rename(&from, &to)?;
            }
        }
        moved.push((then, now));
    }
    if moved.is_empty() {
        return Ok(0);
    }
    let renamed = |id: &str| moved.iter().find(|(then, _)| *then == id).map(|(_, now)| *now);
    let mut changed = moved.len();
    changed += super::super::edit_configs(tx, ConfigKind::Workspace, |_, w| {
        let mut any = false;
        if let Some(now) = w.get("lead").and_then(Value::as_str).and_then(renamed) {
            w.insert("lead".into(), json!(now));
            any = true;
        }
        for m in w.get_mut("members").and_then(Value::as_array_mut).into_iter().flatten() {
            if let Some(now) = m["role"].as_str().and_then(renamed) {
                m["role"] = json!(now);
                any = true;
            }
        }
        any
    })?;
    changed += super::super::edit(tx, super::super::Target::RunMember, |m| {
        match m.get("role_id").and_then(Value::as_str).and_then(renamed) {
            Some(now) => {
                m.insert("role_id".into(), json!(now));
                true
            }
            None => false,
        }
    })?;
    Ok(changed)
}

/// A default role that came after the data was first seeded, made as a first start would make it
/// (`sync_seeds` keeps its text current afterwards). Data never seeded gets it on its first start
/// instead. A role the person already has under this id stays theirs, unless it is the very text
/// shipped (one they copied in by hand): that one becomes the default role. One deleted later
/// stays deleted.
pub(crate) fn add_seed(tx: &rusqlite::Transaction, id: &str) -> Result<usize> {
    let seeded: bool = tx
        .query_row("SELECT COUNT(*) FROM configs WHERE kind = 'meta' AND id = 'seeded'", [], |r| r.get::<_, i64>(0))?
        > 0;
    if !seeded {
        return Ok(0);
    }
    let r = seed_once_named(id).expect("a default role");
    let origin = json!({ "id": id, "desc": text_hash(r.desc), "prompt": text_hash(r.prompt) });
    let existing: Option<String> = tx
        .query_row(
            "SELECT data FROM configs WHERE kind = ?1 AND id = ?2",
            rusqlite::params![ConfigKind::Role.as_str(), id],
            |row| row.get(0),
        )
        .optional()?;
    let role = match existing {
        Some(data) => {
            let mut role: Value = serde_json::from_str(&data)?;
            if role.get("seed").is_some() || role["desc"] != r.desc || role["prompt"] != r.prompt {
                return Ok(0);
            }
            role["seed"] = origin;
            role
        }
        None => json!({
            "id": id, "name": r.name, "desc": r.desc, "prompt": r.prompt, "connection": "",
            "model": "", "effort": "", "session": {}, "lead": r.lead, "color": r.color, "init": r.init, "avatar": "", "seed": origin
        }),
    };
    tx.execute(
        "INSERT OR REPLACE INTO configs (kind, id, data, updated_at) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![ConfigKind::Role.as_str(), id, role.to_string(), crate::util::now_ms()],
    )?;
    Ok(1)
}
