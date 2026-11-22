//! The registered shape of every object Janus stores, at the latest data version. A change to a
//! shape is a change of data: it comes with a migration (`migrations.rs`) and a new
//! `testdata/data_shape.json`. In tests every `put_config` is checked against these shapes, and
//! the upgrade tests check every object a migrated database holds.
// @project-doc docs/architecture/20261031-data_model.md#migrations

use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

/// A setup object stored as plain JSON: the fields it always has and the ones it may have.
pub struct Shape {
    pub kind: &'static str,
    pub required: &'static [&'static str],
    pub optional: &'static [&'static str],
}

pub const SHAPES: &[Shape] = &[
    Shape {
        kind: "role",
        required: &[
            "id",
            "name",
            "desc",
            "prompt",
            "connection",
            "model",
            "effort",
            "session",
            "lead",
            "color",
            "init",
            // The hash of its picture (`data/roles/<id>/avatar.png`); empty shows the initials.
            "avatar",
        ],
        // Default roles remember which default they came from (see `agents::sync_seeds`).
        optional: &["seed"],
    },
    Shape { kind: "workspace", required: &["id", "name", "path", "lead", "members", "brief"], optional: &[] },
    // A path of the person's home shared with agents (`host_env`), stored under its own id.
    Shape { kind: "host_link", required: &["path"], optional: &[] },
    Shape {
        kind: "connection",
        required: &["id", "kind", "base", "name", "auth", "bin", "prompt", "env", "status", "createdAt"],
        // Plus the plain (non-secret) fields of the kind's sign-in methods; see `connection_fields`.
        optional: &[
            "importedEnv",
            "testStartedAt",
            "check",
            "checkedAt",
            "elapsedMs",
            "error",
            "errorLog",
            "agentInfo",
            "options",
            "reply",
            "login",
        ],
    },
];

/// Fields of a workspace's member entries.
const MEMBER_ENTRY: (&[&str], &[&str]) = (&["role"], &["slots"]);

/// The plain fields the agent catalog lets a connection store (e.g. `source`, `baseUrl`).
fn connection_fields() -> Vec<String> {
    let mut out: Vec<String> = crate::agents::kinds()
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|k| k["auths"].as_array().cloned().unwrap_or_default())
        .flat_map(|a| a["fields"].as_array().cloned().unwrap_or_default())
        .filter(|f| f["type"] != "secret")
        .filter_map(|f| f["key"].as_str().map(str::to_owned))
        .collect();
    out.sort();
    out.dedup();
    out
}

/// What is wrong with a stored object of `kind`, in words; empty when it has the latest shape.
pub fn check(kind: &str, data: &Value) -> Vec<String> {
    match kind {
        "library_item" => round_trip::<crate::library::Item>(data),
        "library_source" => round_trip::<crate::library::Source>(data),
        // A workspace Loop (`orchestrator/loops.rs`).
        "loop" => round_trip::<crate::orchestrator::LoopDef>(data),
        "meta" => vec![],
        _ => match SHAPES.iter().find(|s| s.kind == kind) {
            Some(shape) => {
                let extra = if kind == "connection" { connection_fields() } else { vec![] };
                let extra: Vec<&str> = extra.iter().map(String::as_str).collect();
                let mut problems = fields(data, shape.required, &[shape.optional, &extra].concat());
                if kind == "connection"
                    && !data["env"].as_object().is_some_and(|env| {
                        env.values().all(|v| v.is_string() || (data["kind"] == "claude" && v.is_null()))
                    })
                {
                    problems.push("env 的值必须是字符串，Claude 的私有变量用 null 记录变量名".into());
                }
                if kind == "workspace" {
                    for m in data["members"].as_array().into_iter().flatten() {
                        problems.extend(
                            fields(m, MEMBER_ENTRY.0, MEMBER_ENTRY.1).into_iter().map(|p| format!("members: {p}")),
                        );
                    }
                }
                problems
            }
            None => vec![format!("没有登记的设置类型 {kind}：在 data_shape::SHAPES 中登记它的字段")],
        },
    }
}

/// What is wrong with one member of a run's snapshot.
pub fn check_member(data: &Value) -> Vec<String> {
    round_trip::<crate::orchestrator::Member>(data)
}

fn fields(data: &Value, required: &[&str], optional: &[&str]) -> Vec<String> {
    let Some(o) = data.as_object() else { return vec!["不是 JSON 对象".into()] };
    let mut problems: Vec<String> =
        required.iter().filter(|k| !o.contains_key(**k)).map(|k| format!("缺少字段 {k}")).collect();
    problems.extend(
        o.keys()
            .filter(|k| !required.contains(&k.as_str()) && !optional.contains(&k.as_str()))
            .map(|k| format!("多出字段 {k}")),
    );
    problems
}

/// A struct-backed object has the latest shape when reading and writing it gives it back
/// unchanged: a missing field would be filled in, an unknown one dropped.
fn round_trip<T: Serialize + DeserializeOwned>(data: &Value) -> Vec<String> {
    let parsed = match serde_json::from_value::<T>(data.clone()) {
        Ok(v) => v,
        Err(e) => return vec![format!("读不出来：{e}")],
    };
    let back = serde_json::to_value(parsed).unwrap_or(Value::Null);
    if &back == data {
        return vec![];
    }
    let (Some(a), Some(b)) = (data.as_object(), back.as_object()) else { return vec!["写回后不同".into()] };
    let mut problems: Vec<String> = b.keys().filter(|k| !a.contains_key(*k)).map(|k| format!("缺少字段 {k}")).collect();
    problems.extend(a.keys().filter(|k| !b.contains_key(*k)).map(|k| format!("多出字段 {k}")));
    problems.extend(
        a.iter().filter(|(k, v)| b.get(*k).is_some_and(|w| w != *v)).map(|(k, _)| format!("字段 {k} 写回后不同")),
    );
    problems
}

/// The data shape as `testdata/data_shape.json` records it: the data version, the tables a fresh
/// database ends up with, and the registered fields of each kind.
pub fn snapshot() -> anyhow::Result<Value> {
    let mut conn = rusqlite::Connection::open_in_memory()?;
    crate::migrations::upgrade(&mut conn, None)?;
    let tables: Vec<String> = conn
        .prepare("SELECT sql FROM sqlite_master WHERE sql IS NOT NULL ORDER BY type, name")?
        .query_map([], |r| r.get::<_, String>(0))?
        .map(|s| s.map(|s| s.split_whitespace().collect::<Vec<_>>().join(" ")))
        .collect::<rusqlite::Result<_>>()?;
    let kinds: Value = SHAPES
        .iter()
        .map(|s| (s.kind.to_string(), json!({ "required": s.required, "optional": s.optional })))
        .collect::<serde_json::Map<_, _>>()
        .into();
    Ok(json!({
        "dataVersion": crate::migrations::latest(),
        "tables": tables,
        "kinds": kinds,
        "connectionFields": connection_fields(),
        "workspaceMembers": { "required": MEMBER_ENTRY.0, "optional": MEMBER_ENTRY.1 },
    }))
}
