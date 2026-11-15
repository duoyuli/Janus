//! Opening an ACP session for a member: MCP servers, the system prompt, and the controls the role
//! asks for.

use super::*;

/// The values a server declares, by name, from a skipped map. Only the keys the entry carries
/// are passed, so a value no longer in the manifest does not reach the agent.
pub(super) fn named_values(entry: &Value, keys: &str, values: &Value) -> Vec<Value> {
    let saved = &values[entry["name"].as_str().unwrap_or("")];
    entry[keys]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter_map(|k| saved[k].as_str().filter(|v| !v.is_empty()).map(|v| json!({ "name": k, "value": v })))
        .collect()
}

/// Whether the agent declared image input (`promptCapabilities.image`) when it started.
pub(super) fn takes_images(conn: &AcpConn) -> bool {
    conn.capabilities["promptCapabilities"]["image"] == true
}

/// Open a session with the member's MCP servers, in ACP's `McpServer` shape: stdio servers from
/// the role and the library, plus library resources reached over streamable HTTP. Agents that
/// declare no HTTP support (Pi) get only the stdio ones; their session still opens.
pub(super) fn mcp_servers(m: &Member, http: bool) -> Vec<Value> {
    m.mcp
        .iter()
        .filter(|s| s["name"].as_str().is_some_and(|n| !n.is_empty()))
        .filter(|s| http || s["transport"].as_str() != Some("http"))
        .map(|s| {
            if s["transport"].as_str() == Some("http") {
                json!({
                    "type": "http", "name": s["name"], "url": s["url"],
                    "headers": named_values(s, "headerKeys", &m.mcp_headers),
                })
            } else {
                json!({
                    "name": s["name"], "command": s["command"], "args": s["args"],
                    "env": named_values(s, "envKeys", &m.mcp_env),
                })
            }
        })
        .collect()
}

/// Swallow the history a `session/load` replays. Updates may trail the response, so wait until
/// the agent has been quiet for a moment; anything it asks meanwhile is declined.
pub(super) async fn drain_replay(conn: &AcpConn, rx: &mut mpsc::UnboundedReceiver<Incoming>) {
    let quiet = Duration::from_millis(500);
    let until = tokio::time::Instant::now() + Duration::from_secs(10);
    while tokio::time::Instant::now() < until {
        match tokio::time::timeout(quiet, rx.recv()).await {
            Ok(Some(Incoming::Notification { .. })) => {}
            Ok(Some(Incoming::Request { id, .. })) => {
                let _ = conn.respond_error(id, "session is still loading").await;
            }
            Ok(Some(Incoming::Closed)) | Ok(None) | Err(_) => break,
        }
    }
}

pub(super) async fn new_session(conn: &AcpConn, cwd: &Path, m: &Member, extra: &[Value], system: Option<&str>) -> Result<Value> {
    let mut servers = mcp_servers(m, conn.capabilities["mcpCapabilities"]["http"] == true);
    servers.extend_from_slice(extra);
    let meta = with_system_prompt(session_meta(), system);
    let r = conn
        .request(
            "session/new",
            json!({ "cwd": cwd, "mcpServers": servers, "_meta": meta }),
        )
        .await?;
    r["sessionId"]
        .as_str()
        .context("session/new 没有返回 sessionId")?;
    Ok(r)
}

/// Claude Code gets Janus's own board tools allowed without asking (`mcp__janus`): they only read
/// and write the run's board, and in its default mode Claude Code would otherwise ask the person
/// every time a collaborator posts progress. Codex reads the tools' read-only annotations for the
/// same effect. Nothing else changes what the role's mode allows.
// @project-doc docs/interfaces/acp_integration.md#session_meta
pub fn session_meta() -> Value {
    json!({
        "claudeCode": { "options": { "settings": { "permissions": { "allow": ["mcp__janus"] } } } },
    })
}

/// Claude Code appends this to its own system prompt (never replaces it); other agents ignore it.
pub(super) fn with_system_prompt(mut meta: Value, system: Option<&str>) -> Value {
    if let Some(text) = system {
        meta["systemPrompt"] = json!({ "append": text });
    }
    meta
}

/// The control values a session of this member opens with: exactly what its role chose. Read
/// tasks get no override; they run in a disposable snapshot whatever the mode.
pub(super) fn session_values(m: &Member) -> Value {
    match &m.session {
        Value::Object(o) => Value::Object(o.clone()),
        _ => json!({}),
    }
}

/// Apply the role's model, effort and session controls (permission mode and whatever else its
/// agent declares). Returns what was applied, so the attempt records the exact settings it ran with.
// @project-doc docs/interfaces/acp_integration.md#session_setup
pub(super) async fn configure_session(
    conn: &AcpConn,
    session: &Value,
    m: &Member,
) -> Result<Value> {
    let sid = session["sessionId"].as_str().unwrap_or_default();
    let mut opts = crate::agents::session_options(session);
    let mut applied = json!({ "controls": {} });
    for (cat, want, required) in [
        ("model", m.model.as_str(), true),
        ("effort", m.effort.as_str(), false),
    ] {
        let opt = opts[cat].clone();
        if opt.is_null() {
            continue;
        }
        let current = opt["current"].as_str().unwrap_or_default();
        if want.is_empty() || want == current {
            applied[cat] = json!(current);
            continue;
        }
        if !opt["options"]
            .as_array()
            .is_some_and(|o| o.iter().any(|x| x["value"] == want))
        {
            if required {
                bail!(
                    "{} 不提供模型 {want}，请在角色设置中重新选择",
                    m.runtime_name
                );
            }
            applied[cat] = json!(current);
            continue;
        }
        match conn
            .request(
                "session/set_config_option",
                json!({ "sessionId": sid, "configId": opt["id"], "value": want }),
            )
            .await
        {
            Ok(r) => {
                applied[cat] = json!(want);
                // A model switch can change what else is on offer (Claude Code drops Auto on some models).
                if r["configOptions"].is_array() {
                    opts = crate::agents::session_options(&r);
                }
            }
            Err(err) if required => return Err(err.context(format!("设置模型 {want} 失败"))),
            Err(_) => applied[cat] = json!(current),
        }
    }
    let wants = session_values(m);
    for c in opts["controls"].as_array().cloned().unwrap_or_default() {
        let id = c["id"].as_str().unwrap_or_default();
        let current = c["current"].clone();
        let mut want = wants.get(id).cloned().unwrap_or(Value::Null);
        // A session that starts in a plan mode (the agent's own default) moves to the first other option.
        if want.is_null() && crate::agents::is_plan_value(&current) {
            want = c["options"][0]["value"].clone();
        }
        let offered = match &want {
            Value::Bool(_) => c["type"] == "boolean",
            Value::String(_) => c["options"]
                .as_array()
                .is_some_and(|o| o.iter().any(|x| x["value"] == want)),
            _ => false,
        };
        if want.is_null() || want == current || !offered {
            if c["permission"] == true || !want.is_null() {
                applied["controls"][id] = current;
            }
            continue;
        }
        let res = if c["via"] == "mode" {
            conn.request(
                "session/set_mode",
                json!({ "sessionId": sid, "modeId": want }),
            )
            .await
        } else {
            conn.request(
                "session/set_config_option",
                json!({ "sessionId": sid, "configId": id, "value": want }),
            )
            .await
        };
        applied["controls"][id] = if res.is_ok() { want } else { current };
    }
    // Plan toggles are switched off; a plan mode never runs (Codex will not pursue /goal in one).
    for t in opts["planToggles"].as_array().cloned().unwrap_or_default() {
        if let Some(off) = crate::agents::plan_off_value(&t) {
            let id = t["id"].as_str().unwrap_or_default();
            if let Err(err) = conn
                .request("session/set_config_option", json!({ "sessionId": sid, "configId": id, "value": off }))
                .await
            {
                bail!("关闭 {} 的 {} 失败：{err}", m.runtime_name, str_of_value(&t["name"], id));
            }
        }
    }
    if let Some(mode) = opts["controls"]
        .as_array()
        .and_then(|a| a.iter().find(|c| c["category"] == "mode"))
    {
        applied["mode"] = applied["controls"][mode["id"].as_str().unwrap_or_default()].clone();
    }
    applied["label"] = json!(applied_label(&opts, &applied["controls"]));
    Ok(applied)
}

pub(super) fn str_of_value<'a>(v: &'a Value, fallback: &'a str) -> &'a str {
    v.as_str().filter(|s| !s.is_empty()).unwrap_or(fallback)
}

/// "Agent · Plan 关闭": the permission controls a session actually runs with, by display name.
pub(super) fn applied_label(opts: &Value, values: &Value) -> String {
    let conn = json!({ "options": opts });
    crate::agents::session_label(values, &conn)
}

/// What limits this session, as shown with the attempt: only the agent's own permission mode.
pub(super) fn enforcement(applied: &Value) -> String {
    let mode = match applied["label"].as_str() {
        Some(l) if !l.is_empty() => format!("权限 {l}"),
        _ => "Agent 没有权限模式".into(),
    };
    format!("{mode}：直接在工作目录中执行，能做什么由 Agent 的权限模式决定")
}
