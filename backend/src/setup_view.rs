//! What the setup screens show, written for the client's language on top of the setup objects:
//! each agent's line, its check as a list of steps, its environment as text and the icons of its
//! permission choices; each digital employee's initials, line and the line of its model (its
//! permission line and the groups the lists show are `agents::readiness`'s). The Mac app and the web client draw these as they come
//! (docs/decisions/20261126-backend_owns_derived_state.md).
// @project-doc docs/architecture/20261031-frontend.md#setup_lines

use crate::lang::{Lang, tr};
use serde_json::{Value, json};

pub mod adapters;
pub mod bill;
pub mod library;
pub mod mcp_draft;
pub mod picks;
#[cfg(test)]
mod tests;

/// The steps of an agent's check, in order, by the name the check records them under.
const CHECK_STEPS: [(&str, &str); 6] = [
    ("运行组件", "Runtime"),
    ("运行时", "Runtime"),
    ("凭据", "Credentials"),
    ("启动 Agent", "Start Agent"),
    ("打开会话", "Open Session"),
    ("试运行", "Trial Run"),
];

/// A label of the agent catalog (`agents::catalog::kinds`) in `lang`.
fn catalog_label(lang: Lang, label: &str) -> String {
    let en = match label {
        "复制本机配置" => "Copy Local Settings",
        "自定义 API" => "Custom API",
        "Claude 订阅" => "Claude Subscription",
        other => other,
    };
    lang.pick(label, en).to_string()
}

fn s(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}

/// The session controls the agent declared.
fn controls(c: &Value) -> Vec<&Value> {
    c["options"]["controls"].as_array().map(|a| a.iter().collect()).unwrap_or_default()
}

fn is_permission(x: &Value) -> bool {
    x["permission"] == true
}

/// The line under an agent's name: its kind, how it signs in, how many models it offers and its
/// permission modes in its own words.
pub fn connection_summary(c: &Value, kind: Option<&Value>, lang: Lang) -> String {
    let mut parts: Vec<String> = vec![];
    if let Some(k) = kind {
        parts.push(s(&k["name"]).to_string());
        if let Some(a) = k["auths"].as_array().and_then(|a| a.iter().find(|a| a["id"] == c["auth"])) {
            parts.push(catalog_label(lang, s(&a["label"])));
        }
    }
    let options = &c["options"];
    if !options["model"].is_null() {
        let n = options["model"]["options"].as_array().map_or(0, Vec::len);
        parts.push(tr!(lang, "{n} 个模型", "Models: {n}"));
    }
    if !options.is_null() {
        let perms: Vec<&Value> = controls(c).into_iter().filter(|x| is_permission(x)).collect();
        parts.push(if options["controls"].is_null() {
            lang.pick("未读取权限模式", "Permission modes not loaded").into()
        } else if perms.is_empty() {
            lang.pick("无权限模式", "No permission modes").into()
        } else {
            perms
                .iter()
                .map(|x| {
                    if x["type"] == "boolean" {
                        s(&x["name"]).to_string()
                    } else {
                        x["options"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .map(|o| s(&o["name"]).to_string())
                            .collect::<Vec<_>>()
                            .join(" / ")
                    }
                })
                .collect::<Vec<_>>()
                .join(lang.pick("；", " · "))
        });
    }
    parts.join(lang.pick("，", ", "))
}

/// The steps of the agent's check as a list to show: each recorded step with how it went, and
/// while the check runs the steps still to come. Empty before any check.
pub fn check_steps(c: &Value, lang: Lang) -> Vec<Value> {
    let done = c["check"].as_array().cloned().unwrap_or_default();
    if done.is_empty() {
        return vec![];
    }
    let testing = c["status"] == "testing";
    CHECK_STEPS
        .iter()
        .filter_map(|(zh, en)| {
            let step = done.iter().find(|d| d["step"] == *zh);
            let state = match step.map(|d| &d["ok"]) {
                Some(Value::Bool(true)) => "ok",
                Some(Value::Bool(false)) => "failed",
                Some(_) => "running",
                None if testing => "pending",
                None => return None,
            };
            Some(json!({
                "step": lang.pick(zh, en),
                "state": state,
                "detail": step.map(|d| s(&d["detail"])).unwrap_or(""),
            }))
        })
        .collect()
}

/// One `KEY=value` line, the value quoted when it has a space or a `#`, or is quoted already.
fn env_line(k: &str, v: &str) -> String {
    let needs = v.chars().any(|c| c.is_whitespace() || c == '#');
    let quoted = v.chars().next().is_some_and(|q| (q == '"' || q == '\'') && v.ends_with(q));
    if needs || quoted { format!("{k}=\"{v}\"") } else { format!("{k}={v}") }
}

/// An agent's environment as the editor's text: one line per variable, by name, a private value
/// not shown (`null`) as its name alone, which keeps it when saved.
pub fn env_text(env: &Value) -> String {
    let mut vars: Vec<(&String, &Value)> = env.as_object().into_iter().flatten().collect();
    vars.sort_by(|a, b| a.0.cmp(b.0));
    vars.iter()
        .map(|(k, v)| match v {
            Value::Null => k.to_string(),
            v => env_line(k, v.as_str().unwrap_or("")),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The variable a line sets or keeps: `export KEY=value`, `KEY=value` or a bare `KEY`.
fn env_name(line: &str) -> Option<&str> {
    let l = line.trim();
    if l.is_empty() || l.starts_with('#') {
        return None;
    }
    let l = l.strip_prefix("export ").map(str::trim).unwrap_or(l);
    let name = l.split('=').next().unwrap_or("").trim();
    (!name.is_empty()).then_some(name)
}

/// The environment text with an auth's template brought in: its lines for variables the text does
/// not set yet go after what is there, so a value already typed or pasted is never replaced.
pub fn with_template(text: &str, template: &str) -> String {
    let have: std::collections::HashSet<&str> = text.lines().filter_map(env_name).collect();
    let add = template.lines().filter(|l| env_name(l).is_some_and(|n| !have.contains(n)));
    let kept = text.trim();
    (!kept.is_empty()).then_some(kept).into_iter().chain(add).collect::<Vec<_>>().join("\n")
}

/// The icon of a permission choice, and whether it lets the agent do more than the others; what a
/// choice means is the agent's own call, so this reads its value and name.
fn permission_icon(o: &Value) -> (&'static str, bool) {
    let v = format!("{} {}", s(&o["value"]), s(&o["name"])).to_lowercase();
    let read_only = ["readonly", "read-only", "read_only", "read only"].iter().any(|p| v.contains(p));
    if read_only {
        ("eye", false)
    } else if ["full", "bypass", "always", "yolo", "danger", "dontask"].iter().any(|p| v.contains(p)) {
        ("exclamationmark.shield", true)
    } else if v.contains("edit") {
        ("pencil", false)
    } else {
        ("checkmark.shield", false)
    }
}

/// The text a sign-in prints, without its terminal colors, and the links in it.
fn login_log(log: &str) -> (String, Vec<String>) {
    let mut clean = String::with_capacity(log.len());
    let mut chars = log.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            while let Some(&n) = chars.peek() {
                chars.next();
                if !(n.is_ascii_digit() || n == ';') {
                    break;
                }
            }
        } else {
            clean.push(c);
        }
    }
    let urls = clean
        .split(|c: char| c.is_whitespace() || c == '<')
        .filter(|w| w.starts_with("http://") || w.starts_with("https://"))
        .map(str::to_string)
        .collect();
    (clean, urls)
}

/// Add what the setup screens show of an agent (`c`, as `App::connections` gives it).
pub fn dress_connection(c: &mut Value, kinds: &Value, lang: Lang) {
    let kind = kinds.as_array().and_then(|k| k.iter().find(|k| k["id"] == c["kind"]));
    c["summary"] = json!(connection_summary(c, kind, lang));
    c["checks"] = json!(check_steps(c, lang));
    c["envText"] = json!(env_text(&c["env"]));
    let login = kind
        .and_then(|k| k["auths"].as_array())
        .and_then(|a| a.iter().find(|a| a["id"] == c["auth"] && a["login"] == true));
    c["loginLabel"] = login.map_or(Value::Null, |a| json!(catalog_label(lang, s(&a["label"]))));
    if let Some(log) = c["loginLog"].as_str() {
        let (clean, urls) = login_log(log);
        c["loginLog"] = json!(clean);
        c["loginUrls"] = json!(urls);
    }
    if let Some(ctrls) = c["options"]["controls"].as_array_mut() {
        for x in ctrls.iter_mut().filter(|x| x["permission"] == true) {
            for o in x["options"].as_array_mut().into_iter().flatten() {
                let (icon, widens) = permission_icon(o);
                o["icon"] = json!(icon);
                o["widens"] = json!(widens);
            }
        }
    }
}

/// A digital employee's initials as its tile shows them: its own, or the first two letters of its
/// name without spaces, in capitals.
pub fn initials(given: &str, name: &str) -> String {
    if !given.is_empty() {
        return given.to_string();
    }
    name.chars().filter(|c| *c != ' ').take(2).collect::<String>().to_uppercase()
}

/// Add what the lists show of a digital employee (`r`): its initials, its line (what it does, its
/// permission choices as `readiness::perm_label` words them, whether it has instructions) and the
/// line of its model.
pub fn dress_role(r: &mut Value, connections: &[Value], lang: Lang) {
    let c = connections.iter().find(|c| c["id"] == r["connection"] && !s(&r["connection"]).is_empty());
    r["displayInit"] = json!(initials(s(&r["init"]), s(&r["name"])));
    let desc = s(&r["desc"]);
    let permissions = crate::agents::readiness::perm_label(r, connections, lang);
    let mut line = vec![if desc.is_empty() {
        lang.pick("还没有填写职责", "No responsibilities specified").to_string()
    } else {
        desc.to_string()
    }];
    line.push(permissions);
    if !s(&r["prompt"]).is_empty() {
        line.push(lang.pick("提示词", "Prompt").into());
    }
    line.retain(|t| !t.is_empty());
    r["line"] = json!(line.join(" · "));
    r["modelLine"] = match c {
        None => Value::Null,
        Some(c) => {
            let current = s(&c["options"]["model"]["current"]);
            let model = match (s(&r["model"]), current) {
                (m, _) if !m.is_empty() => m.to_string(),
                (_, cur) if !cur.is_empty() => cur.to_string(),
                _ => lang.pick("默认模型", "Default Model").to_string(),
            };
            let effort = s(&r["effort"]);
            json!(if effort.is_empty() { model } else { format!("{model}{}", tr!(lang, "，{effort}", ", {effort}")) })
        }
    };
}

/// The line under a digital employee's name in its editor: how its subtasks went in the recent
/// runs, e.g. "最近 27 个任务：10 个子任务 · 首次通过 70% · 成功平均 5.2 分钟". None when it had none.
pub fn stat_line(x: &Value, runs: i64, lang: Lang) -> Option<String> {
    let tasks = x["tasks"].as_i64().filter(|t| *t > 0)?;
    let pct = (x["firstPass"].as_i64().unwrap_or(0) as f64 / tasks as f64 * 100.0).round() as i64;
    let mut parts = vec![
        tr!(lang, "{tasks} 个子任务", "Subtasks: {tasks}"),
        tr!(lang, "首次通过 {pct}%", "Passed first time: {pct}%"),
    ];
    let failed = x["failed"].as_i64().unwrap_or(0);
    if failed > 0 {
        parts.push(tr!(lang, "失败 {failed}", "Failures: {failed}"));
    }
    let unverified = x["verificationFailed"].as_i64().unwrap_or(0);
    if unverified > 0 {
        parts.push(tr!(lang, "验证没通过 {unverified}", "Verification failures: {unverified}"));
    }
    if let Some(m) = x["avgMinutes"].as_f64().filter(|m| *m > 0.0) {
        parts.push(tr!(lang, "成功平均 {m} 分钟", "Average successful run: {m} min"));
    }
    let parts = parts.join(" · ");
    Some(tr!(lang, "最近 {runs} 个任务：{parts}", "Last {runs} runs: {parts}"))
}
