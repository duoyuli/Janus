//! What a person sees of an ACP session beside the agent's words, so a session in Janus reads
//! like the coding agent itself: its thinking, its plan, the tools still running and the files
//! they changed, the commands and modes it offers, and how much of its context it has used.
//! Each session update becomes an event (or nothing, when it repeats what was already said);
//! `orchestrator::on_update` only calls in here.
// @project-doc docs/interfaces/acp_integration.md#session_view

use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;

/// One turn's running state: tool calls under way, thought not yet shown, and what was last
/// reported, so an unchanged plan or command list is not recorded again.
#[derive(Default)]
pub struct Turn {
    pub tools: HashMap<String, Value>,
    pub last_status: i64,
    pub thought: String,
    plan: String,
    commands: String,
}

/// An event to record: kind, one-line text, data.
pub type Shown = (&'static str, String, Value);

/// The updates that are neither the agent's text nor a tool call.
pub fn event_for(u: &Value, turn: &mut Turn) -> Option<Shown> {
    match u["sessionUpdate"].as_str()? {
        "agent_thought_chunk" => {
            if let Some(t) = u["content"]["text"].as_str() {
                turn.thought.push_str(t);
            }
            None
        }
        "plan" => {
            let entries: Vec<Value> = u["entries"]
                .as_array()?
                .iter()
                .map(|e| json!({ "content": e["content"], "status": e["status"].as_str().unwrap_or("pending"), "priority": e["priority"] }))
                .collect();
            let key = serde_json::to_string(&entries).ok()?;
            if key == turn.plan {
                return None;
            }
            turn.plan = key;
            let done = entries.iter().filter(|e| e["status"] == "completed").count();
            Some(("agent.plan", format!("计划：{done}/{} 完成", entries.len()), json!({ "entries": entries })))
        }
        "available_commands_update" => {
            let commands: Vec<Value> = u["availableCommands"]
                .as_array()?
                .iter()
                .filter_map(|c| {
                    let name = c["name"].as_str()?.trim_start_matches('/');
                    Some(json!({ "name": name, "description": c["description"], "hint": c["input"]["hint"] }))
                })
                .collect();
            let key = serde_json::to_string(&commands).ok()?;
            if key == turn.commands {
                return None;
            }
            turn.commands = key;
            Some(("session.commands", format!("可用命令 {} 个", commands.len()), json!({ "commands": commands })))
        }
        "current_mode_update" => {
            let mode = u["currentModeId"].as_str()?;
            Some(("session.control", format!("模式切换为 {mode}"), json!({ "id": "mode", "value": mode, "via": "mode" })))
        }
        "config_option_update" => {
            let controls = crate::agents::session_options(&json!({ "configOptions": u["configOptions"] }));
            Some(("session.controls", "会话选项已更新".into(), controls))
        }
        "usage_update" => Some(("session.usage", String::new(), json!({ "used": u["used"], "size": u["size"], "cost": u["cost"] }))),
        _ => None,
    }
}

/// The tokens a finished turn used, from the `session/prompt` response.
pub fn prompt_usage(res: &Value) -> Option<Shown> {
    let u = res.get("usage").filter(|u| u.is_object())?;
    Some(("session.usage", String::new(), json!({ "inputTokens": u["inputTokens"], "outputTokens": u["outputTokens"], "totalTokens": u["totalTokens"], "cachedReadTokens": u["cachedReadTokens"] })))
}

/// A tool call the agent just started, shown while it runs; `input` is its summary.
pub fn tool_started(id: &str, entry: &Value, input: &str) -> Value {
    json!({ "callId": id, "tool": entry["kind"].as_str().unwrap_or("tool"), "input": input, "status": entry["status"].as_str().unwrap_or("pending"), "live": true })
}

/// The file changes a tool call reported (`diff` content), each side capped so one big rewrite
/// does not swell the log.
pub fn diffs(entry: &Value) -> Vec<Value> {
    const CAP: usize = 12_000;
    let cap = |s: &str| {
        if s.len() <= CAP {
            return s.to_string();
        }
        let end = (0..=CAP).rev().find(|i| s.is_char_boundary(*i)).unwrap_or(0);
        format!("{}\n…", &s[..end])
    };
    entry["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|c| c["type"] == "diff")
        .map(|c| json!({ "path": c["path"], "old": c["oldText"].as_str().map(cap), "new": cap(c["newText"].as_str().unwrap_or_default()) }))
        .collect()
}

/// The session's controls as it opened, with what Janus applied for the role: the model, the
/// reasoning effort and the other selects (a permission mode among them), each with its current
/// value. The app offers them for switching while the session is open.
pub fn controls(session: &Value, applied: &Value) -> Value {
    let mut c = crate::agents::session_options(session);
    for cat in ["model", "effort"] {
        if !c[cat].is_null() && !applied[cat].is_null() {
            c[cat]["current"] = applied[cat].clone();
        }
    }
    if let Some(list) = c["controls"].as_array_mut() {
        for x in list {
            let id = x["id"].as_str().unwrap_or_default().to_string();
            if let Some(v) = applied["controls"].get(&id).filter(|v| !v.is_null()) {
                x["current"] = v.clone();
            }
        }
    }
    c
}

/// Files the person mentioned as `@path` in a message, inside the workspace, as ACP resource
/// links: every agent takes them, and reads the file itself.
pub fn resource_links(text: &str, cwd: &Path) -> Vec<Value> {
    let Ok(root) = cwd.canonicalize() else { return vec![] };
    let mut out: Vec<Value> = vec![];
    for word in text.split_whitespace() {
        let Some(p) = word.strip_prefix('@') else { continue };
        let p = p.trim_end_matches([',', '.', ';', ':', ')', '，', '。', '；', '：', '）']);
        if p.is_empty() || p.starts_with('/') || p.split('/').any(|s| s == "..") {
            continue;
        }
        let Ok(full) = root.join(p).canonicalize() else { continue };
        if !full.starts_with(&root) || !full.is_file() {
            continue;
        }
        let uri = format!("file://{}", full.display());
        if out.iter().any(|x| x["uri"] == uri.as_str()) {
            continue;
        }
        out.push(json!({ "type": "resource_link", "uri": uri, "name": p }));
    }
    out
}

/// Files in the workspace whose path contains `q`, for the `@` menu: Git's tracked and
/// untracked (not ignored) files, through the read-only probe.
pub fn workspace_files(cwd: &Path, q: &str, limit: usize) -> Vec<String> {
    let out = crate::git_probe::read_only(cwd, &["ls-files", "--cached", "--others", "--exclude-standard"]).output();
    let text = out.map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    let q = q.to_lowercase();
    let mut hits: Vec<&str> = text.lines().filter(|l| l.to_lowercase().contains(&q)).collect();
    // Shorter paths first: the file named is more likely than one deep under a same-named folder.
    hits.sort_by_key(|l| (!l.rsplit('/').next().unwrap_or(l).to_lowercase().starts_with(&q), l.len()));
    hits.dedup();
    hits.into_iter().take(limit).map(str::to_owned).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn updates_become_events_once() {
        let mut t = Turn::default();
        let plan = json!({ "sessionUpdate": "plan", "entries": [{ "content": "读代码", "status": "completed", "priority": "high" }, { "content": "改代码", "status": "in_progress" }] });
        let (kind, text, data) = event_for(&plan, &mut t).unwrap();
        assert_eq!((kind, text.as_str()), ("agent.plan", "计划：1/2 完成"));
        assert_eq!(data["entries"][1]["status"], "in_progress");
        assert!(event_for(&plan, &mut t).is_none(), "an unchanged plan is not recorded again");
        let cmds = json!({ "sessionUpdate": "available_commands_update", "availableCommands": [{ "name": "/compact", "description": "压缩上下文" }, { "name": "review", "description": "审查", "input": { "hint": "目标" } }] });
        let (kind, _, data) = event_for(&cmds, &mut t).unwrap();
        assert_eq!(kind, "session.commands");
        assert_eq!(data["commands"][0]["name"], "compact");
        assert_eq!(data["commands"][1]["hint"], "目标");
        assert!(event_for(&cmds, &mut t).is_none());
        assert!(event_for(&json!({ "sessionUpdate": "agent_thought_chunk", "content": { "type": "text", "text": "想一想" } }), &mut t).is_none());
        assert_eq!(t.thought, "想一想");
        let (kind, _, data) = event_for(&json!({ "sessionUpdate": "current_mode_update", "currentModeId": "plan" }), &mut t).unwrap();
        assert_eq!((kind, &data["value"]), ("session.control", &json!("plan")));
        assert!(prompt_usage(&json!({ "stopReason": "end_turn" })).is_none());
        assert_eq!(prompt_usage(&json!({ "usage": { "inputTokens": 10, "outputTokens": 2, "totalTokens": 12 } })).unwrap().2["totalTokens"], 12);
    }

    #[test]
    fn diffs_and_mentions() {
        let entry = json!({ "content": [{ "type": "diff", "path": "/w/a.rs", "oldText": "a", "newText": "b" }, { "type": "content", "content": { "type": "text", "text": "x" } }] });
        assert_eq!(diffs(&entry), vec![json!({ "path": "/w/a.rs", "old": "a", "new": "b" })]);
        let dir = std::env::temp_dir().join(format!("janus-acp-view-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/main.rs"), "").unwrap();
        let links = resource_links("看一下 @src/main.rs， 还有 @missing.rs 和 @../etc/passwd @src/main.rs", &dir);
        assert_eq!(links.len(), 1, "{links:?}");
        assert_eq!(links[0]["name"], "src/main.rs");
        assert!(links[0]["uri"].as_str().unwrap().ends_with("/src/main.rs"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
