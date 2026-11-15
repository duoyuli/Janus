//! The controls an agent's session offers (model, effort, modes, config options), the values a role
//! keeps for them, and the rule that a plan mode never runs.

use super::*;

/// The ACP session config options Janus reads from an agent: model and reasoning effort, plus
/// `controls`, everything else the agent lets a session choose. Each agent declares its own:
/// Codex sandbox presets, Claude Code permission modes, Grok's permission mode and Plan toggle.
/// Roles pick values from `controls`; nothing about them is hard-coded per agent.
// @project-doc docs/domains/agents_and_roles.md#permission_modes
pub fn session_options(session: &Value) -> Value {
    let mut out = json!({ "controls": [] });
    let mut controls = vec![];
    for o in session["configOptions"]
        .as_array()
        .cloned()
        .unwrap_or_default()
    {
        let cat = match o["category"].as_str() {
            Some("model") => "model",
            Some("thought_level") => "effort",
            _ => {
                if let Some(c) = session_control(&o, "config") {
                    controls.push(c);
                }
                continue;
            }
        };
        if !out[cat].is_null() {
            continue;
        }
        out[cat] = json!({
            "id": o["id"], "current": o["currentValue"],
            "options": select_choices(&o["options"])
        });
    }
    // Agents that predate config options report permission modes as ACP session modes.
    let modes = &session["modes"]["availableModes"];
    if !controls.iter().any(|c| c["category"] == "mode") && modes.as_array().is_some_and(|m| !m.is_empty()) {
        controls.push(json!({
            "id": "mode", "name": "Mode", "desc": "", "category": "mode", "type": "select", "via": "mode",
            "current": session["modes"]["currentModeId"], "permission": true,
            "options": modes.as_array().unwrap().iter().map(|m| json!({ "value": m["id"], "name": m["name"], "desc": m["description"] })).collect::<Vec<_>>(),
        }));
    }
    // Janus never runs an agent in a plan mode: plan toggles leave the controls (kept apart so a
    // session can switch them off) and "plan" leaves every select's options.
    let (plans, mut controls): (Vec<Value>, Vec<Value>) = controls.into_iter().partition(is_plan_toggle);
    for c in controls.iter_mut() {
        if let Some(o) = c["options"].as_array_mut() {
            o.retain(|x| !is_plan_value(&x["value"]));
        }
    }
    if let Some(mode) = controls.iter().find(|c| c["category"] == "mode") {
        out["mode"] = json!({ "id": mode["id"], "current": mode["current"], "options": mode["options"] });
    }
    out["controls"] = json!(controls);
    out["planToggles"] = json!(plans);
    out
}

/// A control that switches the agent into a plan mode: Codex's `plan-mode` (an on/off select),
/// Grok's `plan_mode` (a boolean).
// @project-doc docs/domains/agents_and_roles.md#no_plan_mode
pub fn is_plan_toggle(c: &Value) -> bool {
    str_of(c, "id").to_ascii_lowercase().contains("plan")
}

/// The value that switches a plan toggle off, or None when it is already off or has no off value.
pub fn plan_off_value(c: &Value) -> Option<Value> {
    if c["type"] == "boolean" {
        return (c["current"] == true).then_some(json!(false));
    }
    let off = c["options"].as_array()?.iter().map(|o| &o["value"]).find(|v| {
        v.as_str().is_some_and(|v| matches!(v.to_ascii_lowercase().as_str(), "off" | "false" | "disabled" | "no"))
    })?;
    (c["current"] != *off).then(|| off.clone())
}

/// A select value that is a plan mode (Claude Code's `plan`, or an ACP session mode `plan`).
pub fn is_plan_value(v: &Value) -> bool {
    v.as_str().is_some_and(|v| v.to_ascii_lowercase().replace(['-', '_', ' '], "") == "plan")
}

pub(super) fn select_choices(options: &Value) -> Vec<Value> {
    // Options may also come grouped: [{ group, name, options: [...] }].
    let mut out = vec![];
    for x in options.as_array().cloned().unwrap_or_default() {
        if x["options"].is_array() {
            out.extend(select_choices(&x["options"]));
        } else {
            out.push(json!({ "value": x["value"], "name": x["name"], "desc": x["description"] }));
        }
    }
    out
}

/// One session control in the shape roles use. `permission` marks the ones that decide what the
/// agent may do without asking: they are what the role editor shows under 权限.
pub(super) fn session_control(o: &Value, via: &str) -> Option<Value> {
    let id = o["id"].as_str()?;
    let kind = o["type"].as_str().unwrap_or("select");
    if kind != "select" && kind != "boolean" {
        return None;
    }
    let category = o["category"].as_str().unwrap_or("");
    let permission = matches!(category, "mode" | "_permission")
        || matches!(id.replace('-', "_").as_str(), "mode" | "permission_mode" | "plan_mode" | "approval_policy" | "sandbox");
    Some(json!({
        "id": id, "name": o["name"].as_str().unwrap_or(id), "desc": o["description"],
        "category": category, "type": kind, "via": via, "current": o["currentValue"], "permission": permission,
        "options": if kind == "select" { json!(select_choices(&o["options"])) } else { json!([]) },
    }))
}

/// What a session should be opened with for a role: its chosen control values, keeping only the
/// ones that still match a control the agent declares.
pub fn role_session(role: &Value, conn: &Value) -> Value {
    let controls = conn["options"]["controls"].as_array().cloned().unwrap_or_default();
    let mut out = serde_json::Map::new();
    for (k, v) in role["session"].as_object().cloned().unwrap_or_default() {
        // Plan modes are never chosen, whatever a role or a probe asks for.
        if k.to_ascii_lowercase().contains("plan") || is_plan_value(&v) {
            continue;
        }
        let fits = match controls.iter().find(|c| c["id"] == k.as_str()) {
            Some(c) if c["type"] == "boolean" => v.is_boolean(),
            Some(c) => c["options"].as_array().is_some_and(|o| o.iter().any(|x| x["value"] == v)),
            // Not probed yet: keep the choice, the session check decides.
            None => controls.is_empty() && (v.is_string() || v.is_boolean()),
        };
        if fits {
            out.insert(k, v);
        }
    }
    Value::Object(out)
}

/// The model and reasoning effort a role runs with, by display name: the role's choice, or the
/// agent's default (marked as such) when the role left it open.
pub fn model_label(role: &Value, conn: &Value) -> String {
    let pick = |cat: &str, key: &str| -> Option<(String, bool)> {
        let opt = &conn["options"][cat];
        let chosen = str_of(role, key);
        let (value, default) = if chosen.is_empty() { (opt["current"].as_str()?.to_string(), true) } else { (chosen.to_string(), false) };
        let name = opt["options"]
            .as_array()
            .and_then(|o| o.iter().find(|x| x["value"] == value.as_str()))
            .and_then(|x| x["name"].as_str())
            .filter(|n| !n.is_empty())
            .map(str::to_owned)
            .unwrap_or(value);
        Some((name, default))
    };
    let model = pick("model", "model");
    let effort = pick("effort", "effort");
    let mut s = match &model {
        Some((m, _)) => m.clone(),
        None => return String::new(),
    };
    if let Some((e, _)) = &effort {
        s.push_str(&format!("，推理强度 {e}"));
    }
    match (model.map(|m| m.1), effort.map(|e| e.1)) {
        (Some(true), Some(true) | None) => s.push_str("（Agent 默认）"),
        (Some(true), Some(false)) => s.push_str("（模型为 Agent 默认）"),
        (Some(false), Some(true)) => s.push_str("（推理强度为 Agent 默认）"),
        _ => {}
    }
    s
}

/// A human label for a role's permission choices, e.g. "Agent · Plan 开启".
pub fn session_label(session: &Value, conn: &Value) -> String {
    let controls = conn["options"]["controls"].as_array().cloned().unwrap_or_default();
    let mut parts = vec![];
    for c in controls.iter().filter(|c| c["permission"] == true && !is_plan_toggle(c)) {
        let v = session.get(c["id"].as_str().unwrap_or_default()).unwrap_or(&c["current"]);
        let name = str_of(c, "name");
        parts.push(match v {
            Value::Bool(b) => format!("{name} {}", if *b { "开启" } else { "关闭" }),
            v => c["options"]
                .as_array()
                .and_then(|o| o.iter().find(|x| &x["value"] == v))
                .map(|x| str_of(x, "name").to_string())
                .unwrap_or_else(|| v.as_str().unwrap_or_default().to_string()),
        });
    }
    parts.retain(|p| !p.is_empty());
    parts.join(" · ")
}

#[cfg(test)]
mod session_option_tests {
    use super::*;

    #[test]
    fn a_connection_has_one_phase() {
        let codex = |status: &str, login: &str, logged_in: bool| {
            json!({ "base": "codex", "auth": "chatgpt", "status": status, "login": { "state": login }, "loggedIn": logged_in })
        };
        assert_eq!(connection_phase(&codex("ok", "done", true)), "ok");
        assert_eq!(connection_phase(&codex("ok", "running", false)), "logging_in");
        assert_eq!(connection_phase(&codex("ok", "done", false)), "logged_out");
        assert_eq!(connection_phase(&codex("error", "done", true)), "error");
        assert_eq!(connection_phase(&json!({ "base": "openai", "auth": "key", "status": "untested" })), "untested");
    }

    #[test]
    fn preset_variables_are_defaults_the_agent_settings_override() {
        let kinds = kinds();
        let glm = kinds.as_array().unwrap().iter().find(|k| k["id"] == "glm-claude").unwrap();
        let mut env = preset_env(glm, &json!({ "endpoint": "zai" }));
        let get = |env: &Vec<(String, String)>, k: &str| env.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        assert_eq!(get(&env, "ANTHROPIC_BASE_URL").as_deref(), Some("https://api.z.ai/api/anthropic"), "the chosen endpoint");
        override_env(&mut env, "ANTHROPIC_DEFAULT_OPUS_MODEL", "glm-6");
        override_env(&mut env, "API_TIMEOUT_MS", "");
        assert_eq!(get(&env, "ANTHROPIC_DEFAULT_OPUS_MODEL").as_deref(), Some("glm-6"));
        assert_eq!(get(&env, "API_TIMEOUT_MS"), None, "an empty value unsets the default");
        assert_eq!(env.iter().filter(|(n, _)| n == "ANTHROPIC_DEFAULT_OPUS_MODEL").count(), 1);
    }

    #[test]
    fn controls_follow_what_each_agent_declares() {
        // Grok: permission mode under its own category, Plan as a boolean, model and effort apart.
        let grok = session_options(&json!({ "configOptions": [
            { "id": "plan_mode", "name": "Plan", "type": "boolean", "currentValue": false },
            { "id": "permission_mode", "name": "Permission Mode", "category": "_permission", "type": "select", "currentValue": "ask",
              "options": [{ "value": "ask", "name": "Ask Every Time" }, { "value": "always-approve", "name": "Always Approve" }] },
            { "id": "model", "category": "model", "type": "select", "currentValue": "grok-4", "options": [{ "value": "grok-4", "name": "Grok 4" }] },
        ] }));
        let c = grok["controls"].as_array().unwrap();
        assert_eq!(c.len(), 1, "Plan is never a role choice");
        assert_eq!(grok["planToggles"][0]["id"], "plan_mode", "kept apart so a session can switch it off");
        assert!(c.iter().all(|c| c["permission"] == true));
        assert_eq!(grok["model"]["current"], "grok-4");
        let role = json!({ "session": { "permission_mode": "always-approve", "plan_mode": "yes", "gone": "x" } });
        let conn = json!({ "options": grok });
        assert_eq!(role_session(&role, &conn), json!({ "permission_mode": "always-approve" }));
        assert_eq!(session_label(&role_session(&role, &conn), &conn), "Always Approve");
        // Claude Code's Plan permission mode is not offered, and a role that chose it loses it.
        let claude = session_options(&json!({ "configOptions": [{ "id": "mode", "category": "mode", "type": "select", "currentValue": "default",
            "options": [{ "value": "default" }, { "value": "acceptEdits" }, { "value": "plan" }] }] }));
        assert_eq!(claude["controls"][0]["options"].as_array().unwrap().len(), 2);
        assert_eq!(role_session(&json!({ "session": { "mode": "plan" } }), &json!({ "options": claude })), json!({}));
        // Codex: Plan mode is an on/off select; it is switched off, never offered.
        let codex_plan = session_options(&json!({ "configOptions": [{ "id": "plan-mode", "name": "Plan mode", "type": "select", "currentValue": "on",
            "options": [{ "value": "off" }, { "value": "on" }] }] }));
        assert!(codex_plan["controls"].as_array().unwrap().is_empty());
        assert_eq!(plan_off_value(&codex_plan["planToggles"][0]), Some(json!("off")));
        let mut already_off = codex_plan["planToggles"][0].clone();
        already_off["current"] = json!("off");
        assert_eq!(plan_off_value(&already_off), None);

        // Codex: one mode select; a non-permission option stays a plain session option.
        let codex = session_options(&json!({ "configOptions": [
            { "id": "mode", "name": "Mode", "category": "mode", "type": "select", "currentValue": "agent",
              "options": [{ "value": "read-only", "name": "Read-only" }, { "value": "agent", "name": "Agent" }, { "value": "agent-full-access", "name": "Agent (full access)" }] },
            { "id": "fast", "name": "Fast", "type": "boolean", "currentValue": false },
        ] }));
        assert_eq!(codex["mode"]["current"], "agent");
        let fast = codex["controls"].as_array().unwrap().iter().find(|c| c["id"] == "fast").unwrap().clone();
        assert_eq!(fast["permission"], false);

        // Agents that report ACP session modes: they become a control set through session/set_mode.
        let legacy = session_options(&json!({ "modes": { "currentModeId": "default", "availableModes": [{ "id": "default", "name": "Default" }, { "id": "plan", "name": "Plan" }] } }));
        assert_eq!(legacy["controls"][0]["via"], "mode");
        assert_eq!(legacy["controls"][0]["options"].as_array().unwrap().len(), 1, "the plan session mode is not offered");

        // Model and effort as the lead is told them.
        let opts = json!({ "options": {
            "model": { "current": "gpt-6", "options": [{ "value": "gpt-6", "name": "GPT-6" }, { "value": "sol", "name": "GPT-6.1 Sol" }] },
            "effort": { "current": "medium", "options": [{ "value": "medium", "name": "medium" }, { "value": "high", "name": "high" }] } } });
        assert_eq!(model_label(&json!({ "model": "sol", "effort": "high" }), &opts), "GPT-6.1 Sol，推理强度 high");
        assert_eq!(model_label(&json!({}), &opts), "GPT-6，推理强度 medium（Agent 默认）");
        assert_eq!(model_label(&json!({ "model": "sol" }), &opts), "GPT-6.1 Sol，推理强度 medium（推理强度为 Agent 默认）");
        assert_eq!(model_label(&json!({}), &json!({})), "");

        // Pi: nothing declared.
        assert_eq!(session_options(&json!({ "configOptions": [] }))["controls"], json!([]));
    }
}
